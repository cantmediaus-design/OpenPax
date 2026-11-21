use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Packs at or above this format declare `min_format` / `max_format` instead of `pack_format`.
const FIRST_RANGE_FORMAT: u32 = 65;

/// Contents of `pack.mcmeta`. Unknown keys are preserved on round-trip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PackMeta {
    pub pack: PackSection,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PackSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_format: Option<u32>,
    /// A plain string or a text component.
    #[serde(default)]
    pub description: Value,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl PackMeta {
    pub fn new(description: &str, format: u32) -> Self {
        let mut meta = Self {
            pack: PackSection {
                pack_format: None,
                description: Value::String(description.to_owned()),
                extra: Map::new(),
            },
            extra: Map::new(),
        };
        meta.set_format(format);
        meta
    }

    /// The pack's (highest) declared resource format.
    pub fn format(&self) -> Option<u32> {
        let major = |v: &Value| match v {
            Value::Number(n) => n.as_u64(),
            Value::Array(a) => a.first().and_then(Value::as_u64),
            _ => None,
        };
        self.pack
            .extra
            .get("max_format")
            .and_then(major)
            .and_then(|n| u32::try_from(n).ok())
            .or(self.pack.pack_format)
    }

    /// Targets a single format, using whichever keys that format's game version expects.
    pub fn set_format(&mut self, format: u32) {
        let section = &mut self.pack;
        if format >= FIRST_RANGE_FORMAT {
            section.pack_format = None;
            section.extra.remove("supported_formats");
            section.extra.insert("min_format".into(), format.into());
            section.extra.insert("max_format".into(), format.into());
        } else {
            section.pack_format = Some(format);
            section.extra.remove("min_format");
            section.extra.remove("max_format");
        }
    }

    /// The description, if it is a plain string.
    pub fn description_text(&self) -> Option<&str> {
        self.pack.description.as_str()
    }

    pub fn to_json(&self) -> String {
        let mut out = serde_json::to_string_pretty(self).expect("PackMeta is always serialisable");
        out.push('\n');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_unknown_keys() {
        let src = r#"{
            "pack": { "pack_format": 34, "description": [{"text":"Hi","color":"gold"}], "supported_formats": [34, 42] },
            "overlays": { "entries": [] },
            "language": { "xx_xx": { "name": "X", "region": "Y", "bidirectional": false } }
        }"#;
        let meta: PackMeta = serde_json::from_str(src).unwrap();
        assert_eq!(meta.format(), Some(34));
        assert_eq!(meta.description_text(), None);
        let again: Value = serde_json::from_str(&meta.to_json()).unwrap();
        let original: Value = serde_json::from_str(src).unwrap();
        assert_eq!(again, original);
    }

    #[test]
    fn format_keys_follow_version() {
        let mut meta = PackMeta::new("desc", 46);
        assert_eq!(meta.pack.pack_format, Some(46));
        assert!(!meta.pack.extra.contains_key("min_format"));

        meta.set_format(75);
        assert_eq!(meta.pack.pack_format, None);
        assert_eq!(meta.pack.extra["min_format"], 75);
        assert_eq!(meta.pack.extra["max_format"], 75);
        assert_eq!(meta.format(), Some(75));

        meta.set_format(34);
        assert_eq!(meta.format(), Some(34));
        assert!(!meta.pack.extra.contains_key("max_format"));
    }

    #[test]
    fn reads_major_minor_max_format() {
        let meta: PackMeta = serde_json::from_str(
            r#"{"pack":{"description":"x","min_format":[69,0],"max_format":[75,0]}}"#,
        )
        .unwrap();
        assert_eq!(meta.format(), Some(75));
    }
}
