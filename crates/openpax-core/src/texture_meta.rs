use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Contents of a `<texture>.png.mcmeta` file. Unknown sections are preserved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextureMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<Animation>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpolate: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Default duration of each frame in game ticks (1/20 s). Minecraft defaults to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frametime: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<Vec<Frame>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Frame {
    Index(u32),
    Timed {
        index: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time: Option<u32>,
    },
}

impl TextureMeta {
    pub fn is_empty(&self) -> bool {
        self.animation.is_none() && self.extra.is_empty()
    }

    pub fn to_json(&self) -> String {
        let mut out =
            serde_json::to_string_pretty(self).expect("TextureMeta is always serialisable");
        out.push('\n');
        out
    }
}

impl Animation {
    /// Size of one frame for a texture of the given size.
    ///
    /// Without explicit `width`/`height`, frames are squares stacked along the longer axis.
    pub fn frame_size(&self, tex_width: u32, tex_height: u32) -> (u32, u32) {
        let side = tex_width.min(tex_height).max(1);
        (
            self.width.unwrap_or(side).clamp(1, tex_width.max(1)),
            self.height.unwrap_or(side).clamp(1, tex_height.max(1)),
        )
    }

    /// Number of frames physically present in a texture of the given size.
    pub fn frame_count(&self, tex_width: u32, tex_height: u32) -> u32 {
        let (fw, fh) = self.frame_size(tex_width, tex_height);
        ((tex_width / fw) * (tex_height / fh)).max(1)
    }

    /// The playback order as `(frame index, duration in ticks)`.
    pub fn sequence(&self, tex_width: u32, tex_height: u32) -> Vec<(u32, u32)> {
        let default_time = self.frametime.unwrap_or(1).max(1);
        match &self.frames {
            Some(frames) if !frames.is_empty() => frames
                .iter()
                .map(|f| match *f {
                    Frame::Index(index) => (index, default_time),
                    Frame::Timed { index, time } => (index, time.unwrap_or(default_time).max(1)),
                })
                .collect(),
            _ => (0..self.frame_count(tex_width, tex_height))
                .map(|i| (i, default_time))
                .collect(),
        }
    }

    /// The frame index showing at `tick` ticks into (looping) playback.
    pub fn frame_at(&self, tex_width: u32, tex_height: u32, tick: u64) -> u32 {
        let sequence = self.sequence(tex_width, tex_height);
        let total: u64 = sequence.iter().map(|(_, t)| u64::from(*t)).sum();
        let mut t = tick % total.max(1);
        for (index, time) in &sequence {
            if t < u64::from(*time) {
                return *index;
            }
            t -= u64::from(*time);
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_unknown_sections() {
        let src = r#"{
            "animation": { "frametime": 4, "interpolate": true, "frames": [0, {"index": 2, "time": 10}, 1], "future_key": 1 },
            "texture": { "blur": true },
            "gui": { "scaling": { "type": "stretch" } }
        }"#;
        let meta: TextureMeta = serde_json::from_str(src).unwrap();
        let again: Value = serde_json::from_str(&meta.to_json()).unwrap();
        let original: Value = serde_json::from_str(src).unwrap();
        assert_eq!(again, original);
    }

    #[test]
    fn playback_sequence() {
        let meta: TextureMeta = serde_json::from_str(
            r#"{"animation":{"frametime":2,"frames":[0,{"index":2,"time":5},1]}}"#,
        )
        .unwrap();
        let anim = meta.animation.unwrap();
        assert_eq!(anim.frame_size(16, 48), (16, 16));
        assert_eq!(anim.frame_count(16, 48), 3);
        assert_eq!(anim.sequence(16, 48), vec![(0, 2), (2, 5), (1, 2)]);
        assert_eq!(anim.frame_at(16, 48, 0), 0);
        assert_eq!(anim.frame_at(16, 48, 2), 2);
        assert_eq!(anim.frame_at(16, 48, 7), 1);
        assert_eq!(anim.frame_at(16, 48, 9), 0);
    }

    #[test]
    fn implicit_frames() {
        let anim = Animation::default();
        assert_eq!(anim.sequence(16, 64), vec![(0, 1), (1, 1), (2, 1), (3, 1)]);
    }
}
