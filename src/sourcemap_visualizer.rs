use std::{borrow::Cow, fmt::Write};

use crate::SourceMap;

/// The `SourcemapVisualizer` is a helper for sourcemap testing.
/// It print the mapping of original content and final content tokens.
pub struct SourcemapVisualizer<'a, 'sm> {
    code: &'a str,
    sourcemap: &'a SourceMap<'sm>,
}

impl<'a, 'sm> SourcemapVisualizer<'a, 'sm> {
    pub fn new(code: &'a str, sourcemap: &'a SourceMap<'sm>) -> Self {
        Self { code, sourcemap }
    }

    pub fn get_url(&self) -> String {
        let result = self.sourcemap.to_json_string();
        let s = format!("{}\0{}{}\0{}", self.code.len(), self.code, result.len(), result);
        let hash = base64_simd::STANDARD.encode_to_string(s);
        format!("https://evanw.github.io/source-map-visualization/#{hash}")
    }

    pub fn get_text(&self) -> String {
        let mut s = String::new();
        let source_contents = &self.sourcemap.source_contents;
        if self.sourcemap.source_contents.is_empty() {
            s.push_str("[no source contents]\n");
            return s;
        }

        // Build a 1:1 map: index N in the result corresponds to source_id N.
        // `None` entries are preserved so indexing by source_id stays correct
        // even when some sources have no content (the previous filter_map
        // dropped them, which misaligned all later indices).
        let source_contents_lines_map: Vec<Option<Vec<Vec<u16>>>> = source_contents
            .iter()
            .map(|content| content.as_ref().map(|c| Self::generate_line_utf16_tables(c)))
            .collect();

        let output_lines = Self::generate_line_utf16_tables(self.code);

        let tokens = &self.sourcemap.tokens;

        let mut last_source: Option<&str> = None;
        for i in 0..tokens.len() {
            let t = &tokens[i];
            let Some(source_id) = t.get_source_id() else {
                continue;
            };
            let Some(source) = self.sourcemap.get_source(source_id) else { continue };
            let Some(source_lines) =
                source_contents_lines_map.get(source_id as usize).and_then(|opt| opt.as_ref())
            else {
                // No content for this source; skip rather than panic.
                continue;
            };

            // Print source
            if last_source != Some(source) {
                s.push('-');
                s.push(' ');
                s.push_str(source);
                s.push('\n');
                last_source = Some(source);
            }

            // validate token position
            let dst_invalid = t.dst_line as usize >= output_lines.len()
                || (t.dst_col as usize) >= output_lines[t.dst_line as usize].len();
            let src_invalid = t.src_line as usize >= source_lines.len()
                || (t.src_col as usize) >= source_lines[t.src_line as usize].len();
            if dst_invalid || src_invalid {
                writeln!(
                    s,
                    "({}:{}){} --> ({}:{}){}",
                    t.src_line,
                    t.src_col,
                    if src_invalid { " [invalid]" } else { "" },
                    t.dst_line,
                    t.dst_col,
                    if dst_invalid { " [invalid]" } else { "" },
                )
                .unwrap();
                continue;
            }

            // find next dst column or EOL
            let dst_end_col = {
                match tokens.get(i + 1) {
                    Some(t2) if t2.dst_line == t.dst_line => t2.dst_col,
                    _ => output_lines[t.dst_line as usize].len() as u32,
                }
            };

            // find next src column or EOL
            let src_end_col = 'result: {
                for t2 in &tokens[i + 1..] {
                    if t2.get_source_id() == t.get_source_id() && t2.src_line == t.src_line {
                        // skip duplicate or backward
                        if t2.src_col <= t.src_col {
                            continue;
                        }
                        break 'result t2.src_col;
                    }
                    break;
                }
                source_lines[t.src_line as usize].len() as u32
            };

            writeln!(
                s,
                "({}:{}) {:?} --> ({}:{}) {:?}",
                t.src_line,
                t.src_col,
                Self::str_slice_by_token(source_lines, t.src_line, t.src_col, src_end_col),
                t.dst_line,
                t.dst_col,
                Self::str_slice_by_token(&output_lines, t.dst_line, t.dst_col, dst_end_col)
            )
            .unwrap();
        }

        s
    }

    fn generate_line_utf16_tables(content: &str) -> Vec<Vec<u16>> {
        let mut tables = vec![];
        let mut line_byte_offset = 0;
        let bytes = content.as_bytes();
        for (i, ch) in content.char_indices() {
            match ch {
                '\r' | '\n' | '\u{2028}' | '\u{2029}' => {
                    // Handle Windows-specific "\r\n" newlines. `\n` is a single
                    // ASCII byte, so peeking the next byte is correct even when
                    // earlier content contains multi-byte UTF-8.
                    if ch == '\r' && bytes.get(i + 1) == Some(&b'\n') {
                        continue;
                    }
                    tables.push(content[line_byte_offset..=i].encode_utf16().collect::<Vec<_>>());
                    line_byte_offset = i + 1;
                }
                _ => {}
            }
        }
        tables.push(content[line_byte_offset..].encode_utf16().collect::<Vec<_>>());
        tables
    }

    fn str_slice_by_token(buff: &[Vec<u16>], line: u32, start: u32, end: u32) -> Cow<'_, str> {
        let line = line as usize;
        let start = start as usize;
        let end = end as usize;
        let s = &buff[line];
        String::from_utf16(&s[start.min(end).min(s.len())..start.max(end).min(s.len())])
            .unwrap()
            .replace("\r", "")
            .into()
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use crate::Token;

    #[test]
    fn get_url() {
        let sm =
            SourceMap::from_json_string(r#"{"version":3,"sources":[],"names":[],"mappings":""}"#)
                .unwrap();
        let url = SourcemapVisualizer::new("code", &sm).get_url();
        assert!(url.starts_with("https://evanw.github.io/source-map-visualization/#"));
    }

    #[test]
    fn no_source_contents() {
        // Sources present but no `sourcesContent` at all.
        let sm = SourceMap::from_json_string(
            r#"{"version":3,"sources":["a.js"],"names":[],"mappings":"AAAA"}"#,
        )
        .unwrap();
        assert_eq!(SourcemapVisualizer::new("a", &sm).get_text(), "[no source contents]\n");
    }

    #[test]
    fn skips_tokens_without_resolvable_source() {
        // First token has no source id, second points at a source whose content
        // is `None`; both are skipped, and only the third (valid) token prints.
        let sm = SourceMap::new(
            None,
            vec![],
            None,
            vec![Cow::Borrowed("a.js"), Cow::Borrowed("b.js")],
            vec![Some(Cow::Borrowed("hello\n")), None],
            vec![
                Token::new(0, 0, 0, 0, None, None),
                Token::new(0, 1, 0, 0, Some(1), None),
                Token::new(0, 2, 0, 0, Some(0), None),
            ]
            .into_boxed_slice(),
            None,
        );
        let text = SourcemapVisualizer::new("hello\n", &sm).get_text();
        assert!(text.contains("- a.js"), "{text}");
        assert!(!text.contains("- b.js"), "{text}");
    }

    #[test]
    fn handles_crlf_line_endings() {
        // CRLF source content exercises the `\r\n` peek branch in the line table.
        let sm = SourceMap::new(
            None,
            vec![],
            None,
            vec![Cow::Borrowed("a.js")],
            vec![Some(Cow::Borrowed("aa\r\nbb\r\n"))],
            vec![Token::new(0, 0, 0, 0, Some(0), None), Token::new(1, 0, 1, 0, Some(0), None)]
                .into_boxed_slice(),
            None,
        );
        let text = SourcemapVisualizer::new("aa\r\nbb\r\n", &sm).get_text();
        assert!(text.contains("- a.js"), "{text}");
    }

    #[test]
    fn skips_token_with_out_of_range_source() {
        // A token references a source id past the end of `sources`; the
        // visualizer skips it via the `get_source` guard rather than panicking.
        let sm = SourceMap::new(
            None,
            vec![],
            None,
            vec![Cow::Borrowed("a.js")],
            vec![Some(Cow::Borrowed("aa\n"))],
            vec![Token::new(0, 0, 0, 0, Some(5), None)].into_boxed_slice(),
            None,
        );
        assert_eq!(SourcemapVisualizer::new("aa\n", &sm).get_text(), "");
    }
}
