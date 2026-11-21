use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::to_pretty_json;

/// How many `#reference` hops to follow before giving up on a texture variable.
const MAX_TEXTURE_HOPS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Direction {
    pub const ALL: [Direction; 6] = [
        Self::Down,
        Self::Up,
        Self::North,
        Self::South,
        Self::West,
        Self::East,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }

    pub fn normal(self) -> [f32; 3] {
        match self {
            Self::Down => [0.0, -1.0, 0.0],
            Self::Up => [0.0, 1.0, 0.0],
            Self::North => [0.0, 0.0, -1.0],
            Self::South => [0.0, 0.0, 1.0],
            Self::West => [-1.0, 0.0, 0.0],
            Self::East => [1.0, 0.0, 0.0],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Axis; 3] = [Self::X, Self::Y, Self::Z];

    pub fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
        }
    }
}

/// Contents of a model JSON file. Unknown keys (`display`, `overrides`, ...) are preserved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Texture variables: name to resource location or `#other_variable`.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub textures: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elements: Option<Vec<Element>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Editor-only label written by modelling tools; ignored by the game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(serialize_with = "ser_array")]
    pub from: [f32; 3],
    #[serde(serialize_with = "ser_array")]
    pub to: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Rotation>,
    #[serde(default)]
    pub faces: BTreeMap<Direction, Face>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rotation {
    #[serde(default, serialize_with = "ser_num")]
    pub angle: f32,
    #[serde(default = "default_axis")]
    pub axis: Axis,
    #[serde(default = "default_origin", serialize_with = "ser_array")]
    pub origin: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rescale: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Face {
    /// `[u1, v1, u2, v2]` in 0..16 texture units; derived from the element when absent.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "ser_opt_array"
    )]
    pub uv: Option<[f32; 4]>,
    #[serde(default)]
    pub texture: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cullface: Option<String>,
    /// Texture rotation in degrees: 0, 90, 180 or 270.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tintindex: Option<i32>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn default_axis() -> Axis {
    Axis::Y
}

fn default_origin() -> [f32; 3] {
    [8.0, 8.0, 8.0]
}

/// Whole numbers are written without a fractional part (`16`, not `16.0`).
struct Num(f32);

impl Serialize for Num {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0.fract() == 0.0 && self.0.abs() < 1e9 {
            serializer.serialize_i64(self.0 as i64)
        } else {
            serializer.serialize_f32(self.0)
        }
    }
}

fn ser_num<S: Serializer>(value: &f32, serializer: S) -> Result<S::Ok, S::Error> {
    Num(*value).serialize(serializer)
}

fn ser_array<S: Serializer, const N: usize>(
    value: &[f32; N],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(value.iter().map(|v| Num(*v)))
}

fn ser_opt_array<S: Serializer, const N: usize>(
    value: &Option<[f32; N]>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(array) => ser_array(array, serializer),
        None => serializer.serialize_none(),
    }
}

impl ModelFile {
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub fn to_json(&self) -> String {
        let value = serde_json::to_value(self).expect("ModelFile is always serialisable");
        to_pretty_json(&value)
    }

    pub fn elements(&self) -> &[Element] {
        self.elements.as_deref().unwrap_or_default()
    }

    /// Resolves a face texture such as `#side` to a resource location like `minecraft:block/stone`.
    ///
    /// Returns `None` for variables this file does not define (they may come from a parent).
    pub fn resolve_texture(&self, reference: &str) -> Option<String> {
        let mut current = reference;
        for _ in 0..MAX_TEXTURE_HOPS {
            let Some(variable) = current.strip_prefix('#') else {
                return (!current.is_empty()).then(|| current.to_owned());
            };
            current = self.textures.get(variable)?.as_str()?;
        }
        None
    }

    /// A texture variable name not used yet: `0`, `1`, ...
    pub fn unused_texture_name(&self) -> String {
        (0..)
            .map(|i| i.to_string())
            .find(|name| !self.textures.contains_key(name))
            .unwrap()
    }
}

impl Element {
    /// A box with all six faces using `texture` and automatic UVs.
    pub fn cube(from: [f32; 3], to: [f32; 3], texture: &str) -> Self {
        let face = Face {
            texture: texture.to_owned(),
            ..Default::default()
        };
        Self {
            name: None,
            from,
            to,
            rotation: None,
            faces: Direction::ALL
                .into_iter()
                .map(|d| (d, face.clone()))
                .collect(),
            extra: Map::new(),
        }
    }

    pub fn label(&self, index: usize) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("cube {}", index + 1))
    }

    /// The UV Minecraft uses for a face that does not specify one.
    pub fn default_uv(&self, direction: Direction) -> [f32; 4] {
        let [x1, y1, z1] = self.from;
        let [x2, y2, z2] = self.to;
        match direction {
            Direction::Down => [x1, 16.0 - z2, x2, 16.0 - z1],
            Direction::Up => [x1, z1, x2, z2],
            Direction::North => [16.0 - x2, 16.0 - y2, 16.0 - x1, 16.0 - y1],
            Direction::South => [x1, 16.0 - y2, x2, 16.0 - y1],
            Direction::West => [z1, 16.0 - y2, z2, 16.0 - y1],
            Direction::East => [16.0 - z2, 16.0 - y2, 16.0 - z1, 16.0 - y1],
        }
    }
}

impl Default for Rotation {
    fn default() -> Self {
        Self {
            angle: 0.0,
            axis: default_axis(),
            origin: default_origin(),
            rescale: None,
            extra: Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"{
        "credit": "Made by hand",
        "parent": "minecraft:block/block",
        "ambientocclusion": false,
        "textures": { "0": "minecraft:block/stone", "side": "#0", "particle": "#side" },
        "elements": [
            {
                "name": "post",
                "from": [6, 0, 6],
                "to": [10, 12.5, 10],
                "rotation": { "angle": 22.5, "axis": "y", "origin": [8, 8, 8], "rescale": true },
                "shade": false,
                "faces": {
                    "north": { "uv": [0, 0, 4, 12.5], "texture": "#side", "cullface": "north" },
                    "up": { "texture": "#0", "rotation": 90, "tintindex": 0, "future": 1 }
                }
            }
        ],
        "display": { "gui": { "rotation": [30, 225, 0], "scale": [0.625, 0.625, 0.625] } }
    }"##;

    #[test]
    fn round_trip_preserves_everything() {
        let model = ModelFile::from_json(SAMPLE).unwrap();
        let element = &model.elements()[0];
        assert_eq!(element.name.as_deref(), Some("post"));
        assert_eq!(element.to, [10.0, 12.5, 10.0]);
        assert_eq!(element.rotation.as_ref().unwrap().axis, Axis::Y);
        assert_eq!(element.faces[&Direction::Up].rotation, Some(90));
        assert_eq!(element.faces.len(), 2);

        let again: Value = serde_json::from_str(&model.to_json()).unwrap();
        let original: Value = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(again, original);
    }

    #[test]
    fn whole_numbers_are_written_as_integers() {
        let json = ModelFile::from_json(SAMPLE).unwrap().to_json();
        assert!(json.contains(r#""from": [6, 0, 6]"#), "{json}");
        assert!(json.contains(r#""to": [10, 12.5, 10]"#), "{json}");
        assert!(json.contains(r#""angle": 22.5"#), "{json}");
    }

    #[test]
    fn parent_only_model_stays_minimal() {
        let src =
            r#"{"parent":"minecraft:item/generated","textures":{"layer0":"minecraft:item/ruby"}}"#;
        let model = ModelFile::from_json(src).unwrap();
        assert!(model.elements().is_empty());
        let again: Value = serde_json::from_str(&model.to_json()).unwrap();
        assert_eq!(again, serde_json::from_str::<Value>(src).unwrap());
    }

    #[test]
    fn texture_variables_resolve_through_references() {
        let model = ModelFile::from_json(SAMPLE).unwrap();
        assert_eq!(
            model.resolve_texture("#particle").as_deref(),
            Some("minecraft:block/stone")
        );
        assert_eq!(
            model.resolve_texture("block/dirt").as_deref(),
            Some("block/dirt")
        );
        assert_eq!(model.resolve_texture("#missing"), None);

        let looped = ModelFile::from_json(r##"{"textures":{"a":"#b","b":"#a"}}"##).unwrap();
        assert_eq!(looped.resolve_texture("#a"), None);
        assert_eq!(model.unused_texture_name(), "1");
    }

    #[test]
    fn default_uvs_match_the_game() {
        let element = Element::cube([2.0, 0.0, 4.0], [10.0, 6.0, 16.0], "#0");
        assert_eq!(element.default_uv(Direction::Up), [2.0, 4.0, 10.0, 16.0]);
        assert_eq!(element.default_uv(Direction::Down), [2.0, 0.0, 10.0, 12.0]);
        assert_eq!(
            element.default_uv(Direction::North),
            [6.0, 10.0, 14.0, 16.0]
        );
        assert_eq!(
            element.default_uv(Direction::South),
            [2.0, 10.0, 10.0, 16.0]
        );
        assert_eq!(element.default_uv(Direction::West), [4.0, 10.0, 16.0, 16.0]);
        assert_eq!(element.default_uv(Direction::East), [0.0, 10.0, 12.0, 16.0]);
    }
}
