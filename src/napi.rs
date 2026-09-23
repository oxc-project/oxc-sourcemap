use napi_derive::napi;

use crate::{JSONSourceMap, OwnedSourceMap};

/// A raw version 3 source map accepted as transform input.
#[derive(Debug)]
#[napi(object)]
pub struct InputSourceMap {
    pub file: Option<String>,
    pub mappings: String,
    pub names: Vec<String>,
    pub source_root: Option<String>,
    pub sources: Vec<String>,
    pub sources_content: Option<Vec<Option<String>>>,
    pub version: u32,
    pub debug_id: Option<String>,
}

impl TryFrom<InputSourceMap> for OwnedSourceMap {
    type Error = crate::Error;

    fn try_from(source_map: InputSourceMap) -> Result<Self, Self::Error> {
        Self::from_json(JSONSourceMap {
            version: source_map.version,
            file: source_map.file,
            mappings: source_map.mappings,
            source_root: source_map.source_root,
            sources: source_map.sources,
            sources_content: source_map.sources_content,
            names: source_map.names,
            debug_id: source_map.debug_id,
            x_google_ignore_list: None,
        })
    }
}

// Aligned with Rollup's sourcemap input.
//
// <https://github.com/rollup/rollup/blob/766dbf90d69268971feaafa1f53f88a0755e8023/src/rollup/types.d.ts#L80-L89>
//
// ```
// export interface ExistingRawSourceMap {
//  file?: string;
//  mappings: string;
//  names: string[];
//  sourceRoot?: string;
//  sources: string[];
//  sourcesContent?: string[];
//  version: number;
//  x_google_ignoreList?: number[];
// }
// ```
#[napi(object)]
pub struct SourceMap {
    pub file: Option<String>,
    pub mappings: String,
    pub names: Vec<String>,
    pub source_root: Option<String>,
    pub sources: Vec<String>,
    pub sources_content: Option<Vec<String>>,
    pub version: u8,
    #[napi(js_name = "x_google_ignoreList")]
    pub x_google_ignorelist: Option<Vec<u32>>,
}

impl From<crate::SourceMap<'_>> for SourceMap {
    fn from(source_map: crate::SourceMap<'_>) -> Self {
        let json = source_map.to_json();
        Self {
            file: json.file,
            mappings: json.mappings,
            names: json.names,
            source_root: json.source_root,
            sources: json.sources,
            sources_content: json.sources_content.map(|content| {
                content.into_iter().map(Option::unwrap_or_default).collect::<Vec<_>>()
            }),
            version: 3,
            x_google_ignorelist: json.x_google_ignore_list,
        }
    }
}

impl From<crate::OwnedSourceMap> for SourceMap {
    #[inline]
    fn from(source_map: crate::OwnedSourceMap) -> Self {
        Self::from(source_map.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::{InputSourceMap, SourceMap};

    #[test]
    fn from_source_map() {
        let mut inner = crate::SourceMap::new(
            Some("out.js".into()),
            vec!["n0".into()],
            Some("root".into()),
            vec!["a.js".into()],
            vec![Some("content".into())],
            vec![].into_boxed_slice(),
            None,
        );
        inner.set_x_google_ignore_list(vec![0]);

        let napi: SourceMap = inner.into();
        assert_eq!(napi.version, 3);
        assert_eq!(napi.file.as_deref(), Some("out.js"));
        assert_eq!(napi.source_root.as_deref(), Some("root"));
        assert_eq!(napi.names, vec!["n0".to_string()]);
        assert_eq!(napi.sources, vec!["a.js".to_string()]);
        assert_eq!(napi.sources_content, Some(vec!["content".to_string()]));
        assert_eq!(napi.x_google_ignorelist, Some(vec![0]));
    }

    #[test]
    fn null_source_content_becomes_empty_string() {
        // `sourcesContent` entries that are `null` map to an empty string,
        // because the napi shape uses `Vec<String>` (no inner `Option`).
        let inner = crate::SourceMap::new(
            None,
            vec![],
            None,
            vec!["a.js".into(), "b.js".into()],
            vec![Some("x".into()), None],
            vec![].into_boxed_slice(),
            None,
        );
        let napi: SourceMap = inner.into();
        assert_eq!(napi.sources_content, Some(vec!["x".to_string(), String::new()]));
    }

    #[test]
    fn from_owned_source_map() {
        let owned = crate::OwnedSourceMap::default();
        let napi: SourceMap = owned.into();
        assert_eq!(napi.version, 3);
        assert!(napi.sources.is_empty());
        assert_eq!(napi.sources_content, None);
    }

    #[test]
    fn input_source_map_converts_to_owned_source_map() {
        let input = InputSourceMap {
            file: Some("intermediate.js".to_string()),
            mappings: "AAAA".to_string(),
            names: vec![],
            source_root: Some("../src".to_string()),
            sources: vec!["original.ts".to_string()],
            sources_content: Some(vec![None]),
            version: 3,
            debug_id: Some("debug-id".to_string()),
        };

        let source_map = crate::OwnedSourceMap::try_from(input).unwrap();
        assert_eq!(source_map.get_file(), Some("intermediate.js"));
        assert_eq!(source_map.get_source_root(), Some("../src"));
        assert_eq!(source_map.get_source(0), Some("original.ts"));
        assert_eq!(source_map.get_source_content(0), None);
        assert_eq!(source_map.get_debug_id(), Some("debug-id"));
    }

    #[test]
    fn input_source_map_rejects_unsupported_version() {
        let input = InputSourceMap {
            file: None,
            mappings: String::new(),
            names: vec![],
            source_root: None,
            sources: vec![],
            sources_content: None,
            version: 4,
            debug_id: None,
        };

        let error = crate::OwnedSourceMap::try_from(input).unwrap_err();
        assert_eq!(error.to_string(), "JSON parsing error: unsupported source map version: 4");
    }
}
