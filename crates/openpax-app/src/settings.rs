//! Preferences that persist between sessions (stored through eframe's storage).

use openpax_paint::{Palette, Rgba, ToolKind, ToolSettings};
use serde::{Deserialize, Serialize};

use crate::editor::ToolState;
use crate::theme::ThemeChoice;

pub const SETTINGS_KEY: &str = "settings";
pub const SESSIONS_KEY: &str = "sessions";
/// Packs whose open tabs are remembered.
const MAX_SESSIONS: usize = 16;

/// The tabs that were open in one pack, so reopening the pack picks up where it left off.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PackSession {
    /// The pack folder.
    pub pack: String,
    /// Asset paths of the open tabs, in tab order.
    pub tabs: Vec<String>,
    /// The tab that had focus.
    pub active: Option<String>,
}

/// Remembers `session` as the most recent one, replacing any older entry for the same pack.
pub fn remember_session(sessions: &mut Vec<PackSession>, session: PackSession) {
    sessions.retain(|s| s.pack != session.pack);
    if !session.tabs.is_empty() {
        sessions.insert(0, session);
    }
    sessions.truncate(MAX_SESSIONS);
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeChoice,
    pub show_explorer: bool,
    pub show_inspector: bool,
    /// Whether the navigation hint is shown over the 3D viewport.
    pub show_viewport_hint: bool,
    /// Whether the explorer shows a thumbnail grid instead of the folder tree.
    pub explorer_grid: bool,
    /// Whether the first-run tour has been seen (finished or skipped).
    pub tour_done: bool,
    /// Whether confetti bursts when a pack is created or exported.
    pub celebrations: bool,
    /// Rebound keyboard shortcuts: command id to shortcuts like "Cmd+Shift+S".
    pub shortcuts: std::collections::BTreeMap<String, Vec<String>>,
    pub tool: String,
    pub primary: Rgba,
    pub secondary: Rgba,
    pub brush_size: u32,
    pub mirror_x: bool,
    pub mirror_y: bool,
    pub fill_contiguous: bool,
    pub shape_filled: bool,
    pub palette: Vec<Rgba>,
    pub recent_colors: Vec<Rgba>,
    /// The options last used in the Optimize dialogs.
    pub optimize: crate::optimize::OptimizeSettings,
    /// Every pixel tool option (newer than the individual fields above).
    pub tool_settings: Option<ToolSettings>,
}

impl Default for Settings {
    fn default() -> Self {
        let mut settings = Self {
            theme: ThemeChoice::default(),
            show_explorer: true,
            show_inspector: true,
            show_viewport_hint: true,
            explorer_grid: false,
            tour_done: false,
            celebrations: true,
            shortcuts: std::collections::BTreeMap::new(),
            tool: String::new(),
            primary: [0; 4],
            secondary: [0; 4],
            brush_size: 1,
            mirror_x: false,
            mirror_y: false,
            fill_contiguous: true,
            shape_filled: false,
            palette: Vec::new(),
            recent_colors: Vec::new(),
            optimize: crate::optimize::OptimizeSettings::default(),
            tool_settings: None,
        };
        settings.store_tools(&ToolState::default());
        settings
    }
}

impl Settings {
    /// Copies the tool state into the settings so it is saved with them.
    pub fn store_tools(&mut self, tools: &ToolState) {
        self.tool = tools.tool.name().to_owned();
        self.primary = tools.settings.color;
        self.secondary = tools.secondary;
        self.brush_size = tools.settings.size;
        self.mirror_x = tools.settings.mirror_x;
        self.mirror_y = tools.settings.mirror_y;
        self.fill_contiguous = tools.settings.fill_contiguous;
        self.shape_filled = tools.settings.shape_filled;
        self.palette.clone_from(&tools.palette.colors);
        self.recent_colors.clone_from(&tools.recent);
        self.tool_settings = Some(tools.settings.clone());
    }

    /// The tool state these settings describe.
    pub fn tools(&self) -> ToolState {
        let defaults = ToolState::default();
        ToolState {
            tool: ToolKind::ALL
                .into_iter()
                .find(|t| t.name() == self.tool)
                .unwrap_or(defaults.tool),
            settings: {
                let mut s = self.tool_settings.clone().unwrap_or_default();
                s.color = self.primary;
                s.size = self.brush_size.clamp(1, crate::editor::MAX_BRUSH);
                s.mirror_x = self.mirror_x;
                s.mirror_y = self.mirror_y;
                s.fill_contiguous = self.fill_contiguous;
                s.shape_filled = self.shape_filled;
                s
            },
            secondary: self.secondary,
            palette: if self.palette.is_empty() {
                defaults.palette
            } else {
                Palette {
                    colors: self.palette.clone(),
                }
            },
            recent: self.recent_colors.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_state_round_trips_through_json() {
        let mut tools = ToolState {
            tool: ToolKind::Ellipse,
            settings: ToolSettings {
                color: [1, 2, 3, 255],
                size: 5,
                mirror_x: true,
                ..Default::default()
            },
            secondary: [9, 8, 7, 128],
            recent: vec![[4, 4, 4, 255]],
            ..Default::default()
        };
        tools.palette.colors.truncate(3);

        let mut settings = Settings {
            theme: ThemeChoice::Light,
            ..Default::default()
        };
        settings.store_tools(&tools);
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, settings);

        let back = restored.tools();
        assert_eq!(back.tool, ToolKind::Ellipse);
        assert_eq!(back.settings, tools.settings);
        assert_eq!(back.secondary, tools.secondary);
        assert_eq!(back.palette, tools.palette);
        assert_eq!(back.recent, tools.recent);
    }

    #[test]
    fn sessions_keep_one_entry_per_pack_newest_first() {
        let session = |pack: &str, tabs: &[&str]| PackSession {
            pack: pack.to_owned(),
            tabs: tabs.iter().map(|t| (*t).to_owned()).collect(),
            active: tabs.first().map(|t| (*t).to_owned()),
        };
        let mut sessions = Vec::new();
        remember_session(&mut sessions, session("a", &["x.png"]));
        remember_session(&mut sessions, session("b", &["y.png"]));
        remember_session(&mut sessions, session("a", &["z.png"]));
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0], session("a", &["z.png"]));

        // Closing every tab forgets the pack's session.
        remember_session(&mut sessions, session("a", &[]));
        assert_eq!(sessions, [session("b", &["y.png"])]);

        for i in 0..40 {
            remember_session(&mut sessions, session(&i.to_string(), &["t.png"]));
        }
        assert_eq!(sessions.len(), MAX_SESSIONS);
    }

    #[test]
    fn missing_and_unknown_fields_fall_back_to_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"tool":"Nonsense"}"#).unwrap();
        assert!(settings.show_explorer);
        let tools = settings.tools();
        assert_eq!(tools.tool, ToolKind::Pencil);
        assert!(!tools.palette.colors.is_empty());
    }
}
