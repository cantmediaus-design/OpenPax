//! Icon glyphs from the bundled Phosphor font (`assets/Phosphor.ttf`, MIT licensed).
//!
//! Each constant is a one-character string that renders as an icon in any label or button.

use openpax_core::AssetKind;
use openpax_paint::ToolKind;

// Tools
pub const PENCIL: &str = "\u{E3B4}";
pub const ERASER: &str = "\u{E21E}";
pub const FILL: &str = "\u{E392}";
pub const PICKER: &str = "\u{E568}";
pub const LINE: &str = "\u{E6D2}";
pub const RECTANGLE: &str = "\u{E3F0}";
pub const ELLIPSE: &str = "\u{E18A}";
pub const SELECT: &str = "\u{E69A}";
pub const MOVE: &str = "\u{E0A4}";

// Actions
pub const PLUS: &str = "\u{E3D4}";
pub const MINUS: &str = "\u{E32A}";
pub const CLOSE: &str = "\u{E4F6}";
pub const COPY: &str = "\u{E1CA}";
pub const TRASH: &str = "\u{E4A6}";
pub const MERGE_DOWN: &str = "\u{ED3E}";
pub const SAVE: &str = "\u{E248}";
pub const UNDO: &str = "\u{E038}";
pub const REDO: &str = "\u{E036}";
pub const REFRESH: &str = "\u{E094}";
pub const EXPORT: &str = "\u{EAF0}";
pub const SWAP: &str = "\u{E83C}";
pub const SEARCH: &str = "\u{E30C}";
pub const ZOOM_IN: &str = "\u{E310}";
pub const ZOOM_OUT: &str = "\u{E30E}";
pub const FIT: &str = "\u{E0A2}";
pub const FLIP_H: &str = "\u{ED6A}";
pub const FLIP_V: &str = "\u{ED6C}";
pub const RESIZE: &str = "\u{E626}";
pub const HOME: &str = "\u{E2C2}";
pub const HELP: &str = "\u{E3E8}";
pub const FORMAT: &str = "\u{E484}";
pub const REVEAL: &str = "\u{E5DE}";
pub const CLIPBOARD: &str = "\u{E196}";
pub const SCISSORS: &str = "\u{EAE0}";
pub const COMMAND: &str = "\u{E1C4}";
pub const KEYBOARD: &str = "\u{E2D8}";
pub const BOOK: &str = "\u{E0E6}";
pub const RENAME: &str = "\u{E7D8}";
pub const FUNNEL: &str = "\u{E266}";
pub const LIST: &str = "\u{E2F0}";
pub const LIGHTBULB: &str = "\u{E2DC}";
pub const QUIT: &str = "\u{E42A}";
pub const DRAG_HANDLE: &str = "\u{EAE2}";
pub const ARROW_UP: &str = "\u{E08E}";
pub const ARROW_DOWN: &str = "\u{E03E}";
pub const CHECK: &str = "\u{E182}";
pub const MAGIC_WAND: &str = "\u{E6B6}";
pub const SPARKLE: &str = "\u{E6A2}";

// Objects
pub const FOLDER: &str = "\u{E24A}";
pub const FOLDER_OPEN: &str = "\u{E256}";
pub const FOLDER_PLUS: &str = "\u{E258}";
pub const FILE: &str = "\u{E230}";
pub const FILE_TEXT: &str = "\u{E23A}";
pub const FILE_CODE: &str = "\u{E914}";
pub const FILE_ZIP: &str = "\u{E958}";
pub const IMAGE: &str = "\u{E2CA}";
pub const CUBE: &str = "\u{E1DA}";
pub const SOUND: &str = "\u{E44A}";
pub const LANGUAGE: &str = "\u{E4A2}";
pub const FONT: &str = "\u{E6EE}";
pub const PACKAGE: &str = "\u{E390}";
pub const PALETTE: &str = "\u{E6C8}";
pub const LAYERS: &str = "\u{E466}";
pub const ANIMATION: &str = "\u{E792}";
pub const RECENT: &str = "\u{E1A0}";
pub const SIDEBAR: &str = "\u{EAB6}";

// State
pub const EYE: &str = "\u{E220}";
pub const EYE_OFF: &str = "\u{E224}";
pub const GRID: &str = "\u{E296}";
pub const TILING: &str = "\u{E464}";
pub const CARET_RIGHT: &str = "\u{E13A}";
pub const CARET_DOWN: &str = "\u{E136}";
pub const INFO: &str = "\u{E2CE}";
pub const WARNING: &str = "\u{E4E0}";
pub const ERROR: &str = "\u{E4F8}";
pub const SUCCESS: &str = "\u{E184}";
pub const MOON: &str = "\u{E330}";

pub fn tool(tool: ToolKind) -> &'static str {
    match tool {
        ToolKind::Pencil => PENCIL,
        ToolKind::Eraser => ERASER,
        ToolKind::Fill => FILL,
        ToolKind::Picker => PICKER,
        ToolKind::Line => LINE,
        ToolKind::Rectangle => RECTANGLE,
        ToolKind::Ellipse => ELLIPSE,
        ToolKind::Select => SELECT,
        ToolKind::Move => MOVE,
    }
}

pub fn asset(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Texture => IMAGE,
        AssetKind::Model => CUBE,
        AssetKind::Sound => SOUND,
        AssetKind::Lang => LANGUAGE,
        AssetKind::Font => FONT,
        AssetKind::Text => FILE_TEXT,
        AssetKind::Other => FILE,
        _ => FILE_CODE,
    }
}
