use std::borrow::Cow;

use super::*;
use crate::{ConcatSourceMapBuilder, Error, SourceMap, SourceMapBuilder, Token};

const fn p(line: u32, column: u32) -> Position {
    Position::new(line, column)
}

fn bound(from: Position, expression: u32) -> Vec<Binding> {
    vec![Binding::new(from, Some(expression))]
}

fn map_with(names: &[&'static str], sources: &[&'static str]) -> SourceMap<'static> {
    SourceMap::new(
        None,
        names.iter().map(|&name| Cow::Borrowed(name)).collect(),
        None,
        sources.iter().map(|&source| Some(Cow::Borrowed(source))).collect(),
        vec![],
        vec![].into_boxed_slice(),
        None,
    )
}

fn json_map(names: &str, sources: &str, scope_fields: &str) -> String {
    format!(r#"{{"version":3,"names":{names},"sources":{sources},"mappings":""{scope_fields}}}"#)
}

fn decode_strict(json: &str) -> Result<Option<ScopeInfo>, Error> {
    SourceMap::from_json_string(json).unwrap().decode_scope_info(ScopeDecodeMode::Strict)
}

fn decode_lax(json: &str) -> Option<ScopeInfo> {
    SourceMap::from_json_string(json).unwrap().decode_scope_info(ScopeDecodeMode::Lax).unwrap()
}

// The example from the proposal:
//
// ```js
// var x = 1;                   | var _x = 1;
// function z(message) {        | function _z(_m) {
//   let y = 2;                 |   let _y = 2;
//   console.log(message + y);  |   console.log(_m + _y);
// }                            | }
// z("Hello World");            | console.log("Hello World2"); // inlined
// ```
const EXAMPLE_NAMES: &[&str] = &[
    "global",
    "function",
    "z",
    "x",
    "message",
    "y",
    "_x",
    "_z",
    "_m",
    "_y",
    "\"Hello World\"",
    "2",
];

// Worked out by hand from the encoding rules, independently of the encoder.
const EXAMPLE_SCOPES: &str = "BCAAA,DGD,BHBKEC,DEC,CDB,CBR";
const EXAMPLE_RANGES: &str = "ECAAA,GHI,EHBLAC,GJK,FDB,EDBAAA,GLM,IAFA,Fc,FA";

fn example_info() -> ScopeInfo {
    let function_z = OriginalScope {
        start: p(1, 10),
        end: p(4, 1),
        name: Some(2),
        kind: Some(1),
        is_stack_frame: true,
        variables: vec![4, 5],
        children: vec![],
    };
    let global = OriginalScope {
        start: p(0, 0),
        end: p(5, 17),
        kind: Some(0),
        variables: vec![3, 2],
        children: vec![function_z],
        ..OriginalScope::default()
    };
    let z = ScopeRef::new(0, 1);
    ScopeInfo {
        scopes: vec![Some(vec![global])],
        ranges: vec![GeneratedRange {
            start: p(0, 0),
            end: p(5, 28),
            definition: Some(ScopeRef::new(0, 0)),
            bindings: vec![bound(p(0, 0), 6), bound(p(0, 0), 7)],
            children: vec![
                GeneratedRange {
                    start: p(1, 11),
                    end: p(4, 1),
                    definition: Some(z),
                    stack_frame_type: StackFrameType::Original,
                    bindings: vec![bound(p(1, 11), 8), bound(p(1, 11), 9)],
                    ..GeneratedRange::default()
                },
                GeneratedRange {
                    start: p(5, 0),
                    end: p(5, 28),
                    definition: Some(z),
                    call_site: Some(OriginalPosition::new(0, 5, 0)),
                    bindings: vec![bound(p(5, 0), 10), bound(p(5, 0), 11)],
                    ..GeneratedRange::default()
                },
            ],
            ..GeneratedRange::default()
        }],
    }
}

#[test]
fn encode_proposal_example() {
    let mut sm = map_with(EXAMPLE_NAMES, &["file.js"]);
    sm.set_scope_info(&example_info()).unwrap();
    assert_eq!(sm.get_encoded_scopes().unwrap().collect::<Vec<_>>(), [Some(EXAMPLE_SCOPES)]);
    assert_eq!(sm.get_encoded_ranges().unwrap().collect::<Vec<_>>(), [EXAMPLE_RANGES]);

    let json = sm.to_json_string();
    assert!(
        json.contains(&format!(
            r#""scopes":["{EXAMPLE_SCOPES}"],"ranges":["{EXAMPLE_RANGES}"],"mappings":"#
        )),
        "{json}"
    );
    // `to_json` carries the same fields.
    let value = sm.to_json();
    assert_eq!(value.scopes, Some(vec![Some(EXAMPLE_SCOPES.to_string())]));
    assert_eq!(value.ranges, Some(vec![EXAMPLE_RANGES.to_string()]));
}

#[test]
fn decode_proposal_example() {
    let names = serde_json::to_string(EXAMPLE_NAMES).unwrap();
    let json = json_map(
        &names,
        r#"["file.js"]"#,
        &format!(r#","scopes":["{EXAMPLE_SCOPES}"],"ranges":["{EXAMPLE_RANGES}"]"#),
    );
    let info = decode_strict(&json).unwrap().unwrap();
    assert_eq!(info, example_info());

    let inlined = &info.ranges[0].children[1];
    let definition = info.original_scope(inlined.definition.unwrap()).unwrap();
    assert_eq!(definition.name, Some(2));
    assert!(definition.is_stack_frame);

    // The owned `from_json` path decodes the same.
    let owned = SourceMap::from_json(serde_json::from_str(&json).unwrap()).unwrap();
    assert_eq!(owned.decode_scope_info(ScopeDecodeMode::Strict).unwrap().unwrap(), info);
}

/// Exercises every item and encoding branch: several sources with a `null` entry, multiple root
/// scopes, name-only and kind-only scopes, definitions switching sources, hidden frames,
/// sub-range bindings, multi-line ranges and multi-digit VLQs.
fn complex_info() -> ScopeInfo {
    let block = OriginalScope {
        start: p(3, 6),
        end: p(3, 40),
        kind: Some(2),
        variables: vec![7],
        ..OriginalScope::default()
    };
    let f = OriginalScope {
        start: p(2, 4),
        end: p(4, 5),
        name: Some(3),
        kind: Some(1),
        is_stack_frame: true,
        variables: vec![6],
        children: vec![block],
    };
    let g = OriginalScope {
        start: p(5, 0),
        end: p(9, 1),
        name: Some(4),
        is_stack_frame: true,
        ..OriginalScope::default()
    };
    let module_a = OriginalScope {
        start: p(0, 0),
        end: p(10, 0),
        kind: Some(0),
        variables: vec![8, 9],
        children: vec![f, g],
        ..OriginalScope::default()
    };
    let second_root =
        OriginalScope { start: p(11, 0), end: p(12, 3), name: Some(5), ..OriginalScope::default() };
    let h = OriginalScope {
        start: p(100, 2),
        end: p(200, 1000),
        name: Some(10),
        kind: Some(1),
        is_stack_frame: true,
        variables: vec![11, 12, 13],
        children: vec![],
    };
    let module_c = OriginalScope {
        start: p(0, 0),
        end: p(300, 120),
        kind: Some(0),
        variables: vec![14, 8],
        children: vec![h],
        ..OriginalScope::default()
    };

    ScopeInfo {
        scopes: vec![Some(vec![module_a, second_root]), None, Some(vec![module_c])],
        ranges: vec![
            GeneratedRange {
                start: p(0, 0),
                end: p(0, 5000),
                definition: Some(ScopeRef::new(0, 0)),
                bindings: vec![bound(p(0, 0), 15), vec![Binding::new(p(0, 0), None)]],
                children: vec![
                    GeneratedRange {
                        start: p(0, 10),
                        end: p(0, 900),
                        definition: Some(ScopeRef::new(0, 1)),
                        stack_frame_type: StackFrameType::Original,
                        bindings: vec![bound(p(0, 10), 16)],
                        children: vec![GeneratedRange {
                            start: p(0, 20),
                            end: p(0, 40),
                            definition: Some(ScopeRef::new(0, 2)),
                            bindings: vec![vec![
                                Binding::new(p(0, 20), Some(17)),
                                Binding::new(p(0, 25), None),
                                Binding::new(p(0, 30), Some(18)),
                            ]],
                            ..GeneratedRange::default()
                        }],
                        ..GeneratedRange::default()
                    },
                    GeneratedRange {
                        start: p(0, 1000),
                        end: p(0, 2000),
                        definition: Some(ScopeRef::new(2, 1)),
                        stack_frame_type: StackFrameType::Hidden,
                        bindings: vec![
                            bound(p(0, 1000), 11),
                            vec![Binding::new(p(0, 1000), None)],
                            vec![
                                Binding::new(p(0, 1000), Some(19)),
                                Binding::new(p(0, 1500), Some(20)),
                            ],
                        ],
                        ..GeneratedRange::default()
                    },
                    GeneratedRange {
                        start: p(0, 2100),
                        end: p(0, 2200),
                        definition: Some(ScopeRef::new(0, 3)),
                        stack_frame_type: StackFrameType::Original,
                        ..GeneratedRange::default()
                    },
                    GeneratedRange {
                        start: p(0, 2300),
                        end: p(0, 2400),
                        definition: Some(ScopeRef::new(0, 1)),
                        call_site: Some(OriginalPosition::new(2, 150, 7)),
                        bindings: vec![vec![
                            Binding::new(p(0, 2300), Some(21)),
                            Binding::new(p(0, 2350), None),
                        ]],
                        ..GeneratedRange::default()
                    },
                ],
                ..GeneratedRange::default()
            },
            GeneratedRange {
                start: p(1, 0),
                end: p(3, 4),
                children: vec![GeneratedRange {
                    start: p(2, 0),
                    end: p(2, 9),
                    definition: Some(ScopeRef::new(2, 0)),
                    bindings: vec![bound(p(2, 0), 14), bound(p(2, 0), 8)],
                    ..GeneratedRange::default()
                }],
                ..GeneratedRange::default()
            },
        ],
    }
}

const COMPLEX_NAMES: &[&str] = &[
    "module",
    "function",
    "block",
    "f",
    "g",
    "second_root",
    "p",
    "tmp",
    "a",
    "b",
    "h",
    "q",
    "r",
    "s",
    "c",
    "a$1",
    "p$1",
    "t1",
    "t2",
    "s1",
    "s2",
    "x$1",
];

#[test]
fn roundtrip_complex() {
    let info = complex_info();
    let mut sm = map_with(COMPLEX_NAMES, &["a.js", "b.js", "c.js"]);
    sm.set_scope_info(&info).unwrap();
    let encoded_scopes: Vec<_> = sm.get_encoded_scopes().unwrap().collect();
    assert_eq!(encoded_scopes[1], None);

    let json = sm.to_json_string();
    let parsed = SourceMap::from_json_string(&json).unwrap();
    let decoded = parsed.decode_scope_info(ScopeDecodeMode::Strict).unwrap().unwrap();
    assert_eq!(decoded, info);

    // Re-encoding the decoded information is stable.
    let mut again = map_with(COMPLEX_NAMES, &["a.js", "b.js", "c.js"]);
    again.set_scope_info(&decoded).unwrap();
    assert_eq!(again.to_json_string(), json);

    assert_eq!(info.original_scope(ScopeRef::new(0, 2)).unwrap().kind, Some(2));
    assert_eq!(info.original_scope(ScopeRef::new(0, 4)).unwrap().name, Some(5));
    assert!(info.original_scope(ScopeRef::new(0, 5)).is_none());
    assert!(info.original_scope(ScopeRef::new(1, 0)).is_none());
    assert!(info.original_scope(ScopeRef::new(9, 0)).is_none());
}

#[test]
fn binding_normalization() {
    // A variable that only becomes available after the range start, and one that is unavailable
    // throughout, decode to the explicit form with a leading unavailable binding.
    let scope = OriginalScope {
        start: p(0, 0),
        end: p(0, 100),
        variables: vec![0, 1],
        ..OriginalScope::default()
    };
    let range = |bindings| GeneratedRange {
        start: p(0, 0),
        end: p(0, 50),
        definition: Some(ScopeRef::new(0, 0)),
        bindings,
        ..GeneratedRange::default()
    };
    let info = ScopeInfo {
        scopes: vec![Some(vec![scope])],
        ranges: vec![range(vec![vec![], vec![Binding::new(p(0, 10), Some(1))]])],
    };
    let mut sm = map_with(&["a", "b"], &["a.js"]);
    sm.set_scope_info(&info).unwrap();
    assert_eq!(sm.get_encoded_ranges().unwrap().collect::<Vec<_>>(), ["ECAAA,GAA,HBAKC,FyB"]);

    let decoded = sm.decode_scope_info(ScopeDecodeMode::Strict).unwrap().unwrap();
    assert_eq!(
        decoded.ranges,
        [range(vec![
            vec![Binding::new(p(0, 0), None)],
            vec![Binding::new(p(0, 0), None), Binding::new(p(0, 10), Some(1))],
        ])]
    );
}

#[test]
fn decode_relative_call_sites() {
    // Source 0 scopes: "outer" at 2:4 containing the stack frame "inner" at 3:8.
    let scopes = "BACE,BEBI,CBA,CBA";
    for (call_site, expected) in [
        // Column offset from the parent definition's start.
        ("JC", OriginalPosition::new(0, 2, 6)),
        // Line offset from the parent definition's start, absolute column.
        ("JBC", OriginalPosition::new(0, 3, 2)),
        // Explicit parent index.
        ("JABC", OriginalPosition::new(0, 3, 2)),
        // The absolute form.
        ("IAHK", OriginalPosition::new(0, 7, 10)),
    ] {
        let ranges = format!("ECAAA,ECKAC,{call_site},FK,FU");
        let json = json_map(
            r#"[]"#,
            r#"["a.js"]"#,
            &format!(r#","scopes":["{scopes}"],"ranges":["{ranges}"]"#),
        );
        let info = decode_strict(&json).unwrap().unwrap();
        let inlined = &info.ranges[0].children[0];
        assert_eq!(inlined.definition, Some(ScopeRef::new(0, 1)), "{call_site}");
        assert_eq!(inlined.call_site, Some(expected), "{call_site}");
    }

    // The parent index counts enclosing ranges with a definition, nearest first, skipping the
    // range itself. Ranges: "outer" > no definition > "inner" > inlined "inner".
    for (call_site, expected) in [
        // Parent 0 is the enclosing "inner" range, which starts at 3:8.
        ("JBC", OriginalPosition::new(0, 4, 2)),
        // Parent 1 skips the range without a definition and reaches "outer" at 2:4.
        ("JBBC", OriginalPosition::new(0, 3, 2)),
    ] {
        let ranges = format!("ECAAA,EAC,EGCAC,ECCAA,{call_site},FC,FC,FC,FC");
        let json = json_map(
            r#"[]"#,
            r#"["a.js"]"#,
            &format!(r#","scopes":["{scopes}"],"ranges":["{ranges}"]"#),
        );
        let info = decode_strict(&json).unwrap().unwrap();
        let inlined = &info.ranges[0].children[0].children[0].children[0];
        assert_eq!(inlined.start, p(0, 6), "{call_site}");
        assert_eq!(inlined.call_site, Some(expected), "{call_site}");
    }

    // Without an enclosing definition, a relative call site is an error (strict) or dropped.
    let json = json_map(
        r#"[]"#,
        r#"["a.js"]"#,
        &format!(r#","scopes":["{scopes}"],"ranges":["ECKAC,JC,FK"]"#),
    );
    assert!(matches!(decode_strict(&json), Err(Error::InvalidScopes(_))));
    assert_eq!(decode_lax(&json).unwrap().ranges[0].call_site, None);
}

#[test]
fn unknown_and_vendor_items_are_ignored() {
    // `V` vendor extensions and unknown tags (here `K` and `Z`) may appear at any level and carry
    // any number of values. Start items may also carry trailing values for future extensions.
    let json = json_map(
        r#"["vendor", "global"]"#,
        r#"["a.js"]"#,
        r#","scopes":["VAB,BCAACZZ,KAAAA,CBA,V"],"ranges":["ZZZ,EAAA,VAAA,K,FB"]"#,
    );
    let info = decode_strict(&json).unwrap().unwrap();
    assert_eq!(
        info.scopes,
        [Some(vec![OriginalScope {
            start: p(0, 0),
            end: p(1, 0),
            kind: Some(1),
            ..OriginalScope::default()
        }])]
    );
    assert_eq!(
        info.ranges,
        [GeneratedRange { start: p(0, 0), end: p(0, 1), ..Default::default() }]
    );
}

#[test]
fn ranges_segments_reset_state() {
    // Each segment decodes from a fresh state: positions and definition indices are not relative
    // to the previous segment. Relative to it, the second segment's definition would be in source
    // 2 and the third segment would start at 2:10.
    let json = json_map(
        r#"[]"#,
        r#"["a.js", "b.js"]"#,
        r#","scopes":["BAAA,CAK", "BAAA,BAAB,CAB,CAK"],"ranges":["ECACB,FK", "EDBACB,FK", "EBBK,FA", ""]"#,
    );
    let info = decode_strict(&json).unwrap().unwrap();
    assert_eq!(info.ranges.len(), 3);
    assert_eq!(info.ranges[0].definition, Some(ScopeRef::new(1, 1)));
    assert_eq!((info.ranges[1].start, info.ranges[1].end), (p(1, 0), p(1, 10)));
    assert_eq!(info.ranges[1].definition, Some(ScopeRef::new(1, 1)));
    assert_eq!((info.ranges[2].start, info.ranges[2].end), (p(1, 10), p(1, 10)));

    // A segment that starts before the previous one ends is rejected in strict mode only.
    let json = json_map(r#"[]"#, r#"[]"#, r#","ranges":["EAA,FK", "EAE,FA"]"#);
    assert!(matches!(decode_strict(&json), Err(Error::InvalidScopes(_))));
    assert_eq!(decode_lax(&json).unwrap().ranges.len(), 2);
}

#[test]
fn strict_and_lax_modes() {
    // Each case decodes leniently in lax mode, but fails in strict mode.
    let cases = [
        // Kind name index out of bounds.
        (r#","scopes":["BCAAC,CAA"]"#, "kind"),
        // Variable index out of bounds.
        (r#","scopes":["BAAA,DE,CAA"]"#, "variable"),
        // Binding expression index out of bounds.
        (r#","scopes":["BAAA,DA,CAK"],"ranges":["ECAAA,GE,FK"]"#, "binding"),
        // Definition of a missing scope.
        (r#","scopes":["BAAA,CAK"],"ranges":["ECAAC,FK"]"#, "definition"),
        // Definition in a missing source.
        (r#","scopes":["BAAA,CAK"],"ranges":["ECACA,FK"]"#, "definition source"),
        // Call site in a missing source.
        (r#","scopes":["BAAA,CAK"],"ranges":["EAA,ICAA,FK"]"#, "call site source"),
        // Bindings that do not match the definition's variables.
        (r#","scopes":["BAAA,DA,CAK"],"ranges":["ECAAA,GBB,FK"]"#, "binding count"),
        // Bindings without a definition.
        (r#","scopes":["BAAA,CAK"],"ranges":["EAA,GB,FK"]"#, "bindings without definition"),
        // Sub-range binding for a variable without a binding.
        (r#","scopes":["BAAA,DA,CAK"],"ranges":["ECAAA,GB,HBAEB,FK"]"#, "sub-range"),
        // Hidden flag without the stack frame flag.
        (r#","ranges":["EIA,FK"]"#, "hidden"),
        // Variables item outside of any scope.
        (r#","scopes":["DA"]"#, "stray variables"),
        // Empty item.
        (r#","scopes":["BAAA,,CAK"]"#, "empty item"),
        // More `scopes` entries than sources.
        (r#","scopes":["BAAA,CAK", null]"#, "extra entry"),
        // Duplicate call sites.
        (r#","scopes":["BEAA,CAK"],"ranges":["ECAAA,IAAA,IAAA,FK"]"#, "duplicate call site"),
    ];
    for (fields, case) in cases {
        let json = json_map(r#"["a"]"#, r#"["a.js"]"#, fields);
        assert!(decode_strict(&json).is_err(), "{case}");
        assert!(decode_lax(&json).is_some(), "{case}");
    }

    // In lax mode, unusable references become `None`; variable indices are kept.
    let json = json_map(
        r#"["a"]"#,
        r#"["a.js"]"#,
        r#","scopes":["BDAAAC,DCC,CAK"],"ranges":["ECAAA,GBC,FK","EAU,IGAA,FK"]"#,
    );
    let info = decode_lax(&json).unwrap();
    let scope = &info.scopes[0].as_ref().unwrap()[0];
    assert_eq!((scope.name, scope.kind, scope.variables.as_slice()), (Some(0), None, &[1, 2][..]));
    assert_eq!(
        info.ranges[0].bindings,
        [vec![Binding::new(p(0, 0), Some(0))], vec![Binding::new(p(0, 0), None)]]
    );
    assert_eq!(info.ranges[1].call_site, None);
}

#[test]
fn malformed_strings() {
    for (fields, case) in [
        (r#","scopes":["BAAA"]"#, "unclosed scope"),
        (r#","scopes":["CAA"]"#, "end without start"),
        (r#","scopes":["BAA,CAA"]"#, "missing value"),
        (r#","scopes":["BAAA,C!A"]"#, "invalid character"),
        (r#","scopes":["BAAA,CAg"]"#, "dangling continuation"),
        (r#","scopes":["BAAA,CA//////////////"]"#, "overflow"),
        (r#","scopes":["BAAA,CA+/////H"]"#, "position overflow"),
        (r#","ranges":["EAA"]"#, "unclosed range"),
        (r#","ranges":["FA"]"#, "range end without start"),
        (r#","ranges":["EAA,FAAA"]"#, "range end with three values"),
        (r#","ranges":["EAA,F"]"#, "range end without values"),
        (r#","ranges":["EAA,HA,FA"]"#, "sub-range binding without bindings"),
        (r#","ranges":["EAA,HAA,FA"]"#, "incomplete sub-range binding"),
        (r#","ranges":["EAA,JAAAA,FA"]"#, "relative call site with four values"),
    ] {
        let json = json_map(r#"[]"#, r#"["a.js"]"#, fields);
        assert!(decode_strict(&json).is_err(), "{case}");
        // A string that fails to parse contributes nothing in lax mode.
        let info = decode_lax(&json).unwrap();
        assert_eq!(info.scopes, [None], "{case}");
        assert!(info.ranges.is_empty(), "{case}");
    }

    // A malformed ranges segment drops all ranges, including those of valid segments.
    let json = json_map(r#"[]"#, r#"[]"#, r#","ranges":["EAA,FA","EAA"]"#);
    assert!(decode_lax(&json).unwrap().ranges.is_empty());
}

#[test]
fn absent_and_empty_fields() {
    let json = json_map(r#"[]"#, r#"["a.js"]"#, "");
    assert_eq!(decode_strict(&json).unwrap(), None);

    // A `null` entry has no scope information, an empty string has no scopes.
    let json = json_map(r#"[]"#, r#"["a.js", "b.js", "c.js"]"#, r#","scopes":[null, ""]"#);
    let info = decode_strict(&json).unwrap().unwrap();
    assert_eq!(info.scopes, [None, Some(vec![]), None]);
    assert!(info.ranges.is_empty());

    let json = json_map(r#"[]"#, r#"["a.js"]"#, r#","ranges":[]"#);
    assert_eq!(
        decode_strict(&json).unwrap().unwrap(),
        ScopeInfo { scopes: vec![None], ranges: vec![] }
    );
}

#[test]
fn json_fields_are_lenient() {
    // Shapes that the spec asks consumers to ignore must not fail parsing the whole map.
    for (fields, scopes, ranges) in [
        // The single-string encoding of earlier drafts is ignored.
        (r#","scopes":"BCAAA,CAA""#, None, None),
        (r#","scopes":42,"ranges":{"a":[1]}"#, None, None),
        (r#","scopes":null,"ranges":null"#, None, None),
        // Non-string entries read as `null` scopes and as empty segments.
        (
            r#","scopes":[1, true, {"a":1}, [2], "", null],"ranges":[1, "EAA,FA"]"#,
            Some(vec![None, None, None, None, Some(""), None]),
            Some(vec!["", "EAA,FA"]),
        ),
        // A single string is one ranges segment.
        (r#","ranges":"EAA,FA""#, None, Some(vec!["EAA,FA"])),
        // Escaped strings are unescaped.
        (
            r#","scopes":["BAAA,CAA"],"ranges":["EAA,FA"]"#,
            Some(vec![Some("BAAA,CAA")]),
            Some(vec!["EAA,FA"]),
        ),
    ] {
        let json = json_map(r#"[]"#, r#"["a.js"]"#, fields);
        let sm = SourceMap::from_json_string(&json).unwrap();
        assert_eq!(sm.get_encoded_scopes().map(Iterator::collect), scopes, "{fields}");
        assert_eq!(sm.get_encoded_ranges().map(Iterator::collect), ranges, "{fields}");

        let owned: crate::JSONSourceMap = serde_json::from_str(&json).unwrap();
        assert_eq!(
            owned.scopes,
            scopes.as_ref().map(|s| s.iter().map(|s| s.map(str::to_owned)).collect()),
            "{fields}"
        );
        assert_eq!(
            owned.ranges,
            ranges.as_ref().map(|r| r.iter().map(|&r| r.to_owned()).collect()),
            "{fields}"
        );
    }
}

#[test]
fn unescaped_fields_are_borrowed() {
    let json = json_map(r#"[]"#, r#"["a.js"]"#, r#","scopes":["BAAA,CAA"],"ranges":["EAA,FA"]"#);
    let sm = SourceMap::from_json_string(&json).unwrap();
    assert!(matches!(sm.scopes.as_deref(), Some([Some(Cow::Borrowed(_))])));
    assert!(matches!(sm.ranges.as_deref(), Some([Cow::Borrowed(_)])));

    // Detaching keeps the fields.
    let owned = sm.into_owned();
    assert_eq!(owned.get_encoded_scopes().unwrap().collect::<Vec<_>>(), [Some("BAAA,CAA")]);
    let parts = owned.into_parts();
    assert_eq!(parts.ranges.as_deref(), Some(&[Cow::Borrowed("EAA,FA")][..]));
    let rebuilt = SourceMap::from_parts(parts);
    assert_eq!(rebuilt.get_encoded_ranges().unwrap().collect::<Vec<_>>(), ["EAA,FA"]);
}

#[test]
fn passthrough_preserves_unknown_items() {
    // Parsing and serializing does not decode, so items this crate ignores survive.
    let fields = r#","scopes":["VAB,BAAA,CAA"],"ranges":["ZZ","EAA,FA"]"#;
    let json = json_map(r#"[]"#, r#"["a.js"]"#, fields);
    let output = SourceMap::from_json_string(&json).unwrap().to_json_string();
    assert!(output.contains(r#""scopes":["VAB,BAAA,CAA"],"ranges":["ZZ","EAA,FA"]"#), "{output}");

    let mut sm = SourceMap::from_json_string(&json).unwrap();
    sm.clear_scope_info();
    assert!(!sm.to_json_string().contains("scopes"));
    assert_eq!(sm.decode_scope_info(ScopeDecodeMode::Strict).unwrap(), None);
}

#[test]
fn encode_validation() {
    let scope = |start, end| OriginalScope { start, end, ..OriginalScope::default() };
    let range = |start, end| GeneratedRange { start, end, ..GeneratedRange::default() };
    let function = OriginalScope {
        start: p(0, 0),
        end: p(0, 10),
        is_stack_frame: true,
        variables: vec![0],
        ..OriginalScope::default()
    };
    let with_function = |range: GeneratedRange| ScopeInfo {
        scopes: vec![Some(vec![function.clone()])],
        ranges: vec![range],
    };
    let defined = GeneratedRange {
        start: p(0, 0),
        end: p(0, 10),
        definition: Some(ScopeRef::new(0, 0)),
        ..GeneratedRange::default()
    };

    let invalid = [
        // Wrong number of `scopes` entries.
        (ScopeInfo { scopes: vec![], ranges: vec![] }, "scopes length"),
        // Unsorted, overlapping and badly nested scopes and ranges.
        (
            ScopeInfo { scopes: vec![Some(vec![scope(p(0, 5), p(0, 1))])], ranges: vec![] },
            "scope end",
        ),
        (
            ScopeInfo {
                scopes: vec![Some(vec![scope(p(1, 0), p(2, 0)), scope(p(0, 0), p(0, 1))])],
                ranges: vec![],
            },
            "unsorted scopes",
        ),
        (
            ScopeInfo {
                scopes: vec![None],
                ranges: vec![GeneratedRange {
                    children: vec![range(p(0, 0), p(0, 20))],
                    ..range(p(0, 0), p(0, 10))
                }],
            },
            "child range outside parent",
        ),
        // References to missing names, sources and scopes.
        (
            ScopeInfo {
                scopes: vec![Some(vec![OriginalScope {
                    name: Some(5),
                    ..scope(p(0, 0), p(0, 1))
                }])],
                ranges: vec![],
            },
            "name",
        ),
        (
            with_function(GeneratedRange {
                definition: Some(ScopeRef::new(0, 1)),
                ..defined.clone()
            }),
            "definition scope",
        ),
        (
            with_function(GeneratedRange {
                definition: Some(ScopeRef::new(1, 0)),
                ..defined.clone()
            }),
            "definition source",
        ),
        (
            with_function(GeneratedRange { bindings: vec![bound(p(0, 0), 9)], ..defined.clone() }),
            "binding name",
        ),
        // Bindings must match the definition.
        (with_function(GeneratedRange { bindings: vec![], ..defined.clone() }), "valid"),
        (
            with_function(GeneratedRange { bindings: vec![vec![], vec![]], ..defined.clone() }),
            "binding count",
        ),
        (
            with_function(GeneratedRange { bindings: vec![vec![]], ..range(p(0, 0), p(0, 10)) }),
            "bindings without definition",
        ),
        (
            with_function(GeneratedRange {
                bindings: vec![vec![Binding::new(p(0, 5), Some(0)), Binding::new(p(0, 2), None)]],
                ..defined.clone()
            }),
            "unsorted bindings",
        ),
        (
            with_function(GeneratedRange {
                bindings: vec![vec![Binding::new(p(0, 11), Some(0))]],
                ..defined.clone()
            }),
            "binding after range",
        ),
        // Inlined ranges.
        (
            with_function(GeneratedRange {
                call_site: Some(OriginalPosition::new(0, 0, 0)),
                ..range(p(0, 0), p(0, 10))
            }),
            "call site without definition",
        ),
        (
            with_function(GeneratedRange {
                call_site: Some(OriginalPosition::new(0, 0, 0)),
                stack_frame_type: StackFrameType::Original,
                ..defined.clone()
            }),
            "inlined stack frame",
        ),
        (
            with_function(GeneratedRange {
                call_site: Some(OriginalPosition::new(3, 0, 0)),
                ..defined.clone()
            }),
            "call site source",
        ),
        (
            ScopeInfo {
                scopes: vec![Some(vec![scope(p(0, 0), p(0, 10))])],
                ranges: vec![GeneratedRange {
                    call_site: Some(OriginalPosition::new(0, 0, 0)),
                    ..defined.clone()
                }],
            },
            "inlined definition is not a stack frame",
        ),
    ];
    for (info, case) in invalid {
        let mut sm = map_with(&["a"], &["a.js"]);
        let result = sm.set_scope_info(&info);
        if case == "valid" {
            assert!(result.is_ok(), "{case}");
            continue;
        }
        assert!(result.is_err(), "{case}");
        assert!(sm.get_encoded_scopes().is_none(), "{case}: map must be unchanged");
    }
}

#[test]
fn builder_set_scope_info() {
    let mut builder = SourceMapBuilder::default();
    for name in EXAMPLE_NAMES {
        builder.add_name(name);
    }
    // Scope information must cover every source.
    assert!(builder.set_scope_info(&example_info()).is_err());
    builder.add_source_and_content("file.js", "");
    builder.set_scope_info(&example_info()).unwrap();

    let sm = builder.into_sourcemap();
    assert_eq!(sm.get_encoded_scopes().unwrap().collect::<Vec<_>>(), [Some(EXAMPLE_SCOPES)]);
    assert_eq!(sm.get_encoded_ranges().unwrap().collect::<Vec<_>>(), [EXAMPLE_RANGES]);
}

#[test]
fn owned_sourcemap_scope_info() {
    let mut owned = map_with(EXAMPLE_NAMES, &["file.js"]).into_owned_sourcemap();
    owned.set_scope_info(&example_info()).unwrap();
    assert_eq!(owned.decode_scope_info(ScopeDecodeMode::Strict).unwrap(), Some(example_info()));
    owned.clear_scope_info();
    assert_eq!(owned.decode_scope_info(ScopeDecodeMode::Lax).unwrap(), None);
}

#[test]
fn concat_shifts_scope_info() {
    // Two copies of the example, a map without scope information in between, and the copies
    // placed at different generated lines.
    let mut first = map_with(EXAMPLE_NAMES, &["file.js"]);
    first.set_scope_info(&example_info()).unwrap();
    let plain = map_with(&["unused"], &["plain.js"]);
    let mut second = map_with(EXAMPLE_NAMES, &["other.js"]);
    second.set_scope_info(&example_info()).unwrap();

    let concat =
        ConcatSourceMapBuilder::from_sourcemaps(&[(&first, 0), (&plain, 10), (&second, 20)])
            .with_scopes(true)
            .into_sourcemap();
    let info = concat.decode_scope_info(ScopeDecodeMode::Strict).unwrap().unwrap();

    // The first map is passed through unchanged.
    assert_eq!(concat.get_encoded_ranges().unwrap().next(), Some(EXAMPLE_RANGES));
    assert_eq!(info.scopes.len(), 3);
    assert_eq!(info.scopes[0], example_info().scopes[0]);
    assert_eq!(info.scopes[1], None);

    // The second map's names move by 13 (12 + "unused"), its source by 2 and its lines by 20.
    let mut expected = example_info();
    shift_scopes(expected.scopes[0].as_mut().unwrap(), 13);
    shift_range(&mut expected.ranges[0], 20, 2, 13);
    assert_eq!(info.scopes[2], expected.scopes[0]);
    assert_eq!(info.ranges.len(), 2);
    assert_eq!(info.ranges[0], example_info().ranges[0]);
    assert_eq!(info.ranges[1], expected.ranges[0]);

    // Shifted names resolve to the same strings.
    let inlined = &info.ranges[1].children[1];
    let message = inlined.bindings[0][0].expression.unwrap();
    assert_eq!(concat.get_name(message), Some("\"Hello World\""));
    assert_eq!(inlined.call_site, Some(OriginalPosition::new(2, 5, 0)));

    // The owned path produces the same result.
    let owned = ConcatSourceMapBuilder::from_owned_sourcemaps(vec![
        (first.clone(), 0),
        (plain.clone(), 10),
        (second.clone(), 20),
    ])
    .with_scopes(true)
    .into_sourcemap();
    assert_eq!(owned.to_json_string(), concat.to_json_string());

    // Enabling applies to maps added before and after the call.
    let mut builder = ConcatSourceMapBuilder::default();
    builder.add_sourcemap(&first, 0);
    let mut builder = builder.with_scopes(true);
    builder.add_sourcemap(&plain, 10);
    builder.add_sourcemap(&second, 20);
    assert_eq!(builder.into_sourcemap().to_json_string(), concat.to_json_string());
}

#[test]
fn concat_scope_info_is_opt_in() {
    let mut first = map_with(EXAMPLE_NAMES, &["file.js"]);
    first.set_scope_info(&example_info()).unwrap();
    let mut second = map_with(EXAMPLE_NAMES, &["other.js"]);
    second.set_scope_info(&example_info()).unwrap();
    let mut first_plain = first.clone();
    first_plain.clear_scope_info();
    let mut second_plain = second.clone();
    second_plain.clear_scope_info();
    let expected =
        ConcatSourceMapBuilder::from_sourcemaps(&[(&first_plain, 0), (&second_plain, 20)])
            .into_sourcemap()
            .to_json_string();

    // By default, or when disabled, scope information is dropped and the output is the same as
    // for inputs without any.
    for concat in [
        ConcatSourceMapBuilder::from_sourcemaps(&[(&first, 0), (&second, 20)]),
        ConcatSourceMapBuilder::from_sourcemaps(&[(&first, 0), (&second, 20)]).with_scopes(false),
        ConcatSourceMapBuilder::from_owned_sourcemaps(vec![
            (first.clone(), 0),
            (second.clone(), 20),
        ]),
        ConcatSourceMapBuilder::from_sourcemaps(&[(&first, 0), (&second, 20)])
            .with_scopes(true)
            .with_scopes(false),
    ] {
        let concat = concat.into_sourcemap();
        assert!(concat.get_encoded_scopes().is_none());
        assert!(concat.get_encoded_ranges().is_none());
        assert_eq!(concat.to_json_string(), expected);
    }
}

#[test]
fn concat_without_scope_info() {
    let a = map_with(&["a"], &["a.js"]);
    let b = map_with(&["b"], &["b.js"]);
    let concat = ConcatSourceMapBuilder::from_sourcemaps(&[(&a, 0), (&b, 1)])
        .with_scopes(true)
        .into_sourcemap();
    assert!(concat.get_encoded_scopes().is_none());
    assert!(concat.get_encoded_ranges().is_none());

    // Sources of maps before the first one with scope information get `null` entries, and
    // malformed scope information of a shifted map is dropped.
    let mut with_scopes = map_with(&[], &["c.js"]);
    with_scopes.scopes = Some(vec![Some(Cow::Borrowed("BAAA,CAK"))]);
    let mut malformed = map_with(&[], &["d.js", "e.js"]);
    malformed.scopes = Some(vec![Some(Cow::Borrowed("BAAA"))]);
    malformed.ranges = Some(vec![Cow::Borrowed("EAA")]);
    let concat =
        ConcatSourceMapBuilder::from_sourcemaps(&[(&a, 0), (&with_scopes, 1), (&malformed, 2)])
            .with_scopes(true)
            .into_sourcemap();
    assert_eq!(
        concat.get_encoded_scopes().unwrap().collect::<Vec<_>>(),
        [None, Some("BAAA,CAK"), None, None]
    );
    assert!(concat.get_encoded_ranges().is_none());
}

fn shift_scopes(scopes: &mut [OriginalScope], name_offset: u32) {
    for scope in scopes {
        scope.name = scope.name.map(|name| name + name_offset);
        scope.kind = scope.kind.map(|kind| kind + name_offset);
        for variable in &mut scope.variables {
            *variable += name_offset;
        }
        shift_scopes(&mut scope.children, name_offset);
    }
}

fn shift_range(range: &mut GeneratedRange, line: u32, source: u32, name: u32) {
    range.start.line += line;
    range.end.line += line;
    if let Some(definition) = &mut range.definition {
        definition.source_index += source;
    }
    if let Some(call_site) = &mut range.call_site {
        call_site.source_index += source;
    }
    for binding in range.bindings.iter_mut().flatten() {
        binding.from.line += line;
        binding.expression = binding.expression.map(|expression| expression + name);
    }
    for child in &mut range.children {
        shift_range(child, line, source, name);
    }
}

#[test]
fn compose_keeps_original_scopes_and_drops_ranges() {
    // `generated` maps the output to an intermediate file, `input` maps that file to the original.
    let mut generated = map_with(&[], &["intermediate.js"]);
    generated.tokens = vec![Token::new(0, 0, 0, 0, Some(0), None)].into_boxed_slice();
    generated.ranges = Some(vec![Cow::Borrowed("EAA,FA")]);
    let mut input = map_with(&[], &["original.js"]);
    input.tokens = vec![Token::new(0, 0, 0, 0, Some(0), None)].into_boxed_slice();
    input.scopes = Some(vec![Some(Cow::Borrowed("BAAA,CAK"))]);
    input.ranges = Some(vec![Cow::Borrowed("EAA,FK")]);

    let composed = generated.compose(input);
    assert_eq!(composed.get_encoded_scopes().unwrap().collect::<Vec<_>>(), [Some("BAAA,CAK")]);
    assert!(composed.get_encoded_ranges().is_none());
}
