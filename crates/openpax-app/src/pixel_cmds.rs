//! Texture-editor commands beyond the basics: image, selection, layer, frame, filter, view and
//! export actions. They plug into [`crate::commands::Command::Pixel`], so they get menus,
//! shortcuts, the shortcut editor and the command palette for free.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};

use crate::icons;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelCmd {
    // Image
    ScaleImage,
    CropToSelection,
    Trim,
    RotateCanvasCw,
    RotateCanvasCcw,
    RotateCanvas180,
    IndexedMode,
    // Select
    InvertSelection,
    SelectLayerAlpha,
    SaveSelection,
    CopyMerged,
    PasteInPlace,
    FillSelection,
    NudgeLeft,
    NudgeRight,
    NudgeUp,
    NudgeDown,
    NudgeLeftFar,
    NudgeRightFar,
    NudgeUpFar,
    NudgeDownFar,
    Commit,
    // Transform
    FlipSelectionH,
    FlipSelectionV,
    RotateCw,
    RotateCcw,
    Rotate180,
    RotateBy,
    ScaleSelection,
    // Layer
    NewGroup,
    GroupLayer,
    Ungroup,
    MergeVisible,
    FlattenLayers,
    LayerViaCopy,
    LayerViaCut,
    AddReference,
    ToggleLayerLock,
    ToggleAlphaLock,
    NewTilemap,
    // Frame
    NewFrame,
    DuplicateFrame,
    NewLinkedFrame,
    DeleteFrame,
    PrevFrame,
    NextFrame,
    FirstFrame,
    LastFrame,
    PlayAnimation,
    ToggleOnionSkin,
    AddTag,
    LinkCels,
    UnlinkCel,
    // Filters
    Filters,
    Outline,
    ReplaceColor,
    PaletteSwap,
    ReduceColors,
    Invert,
    Desaturate,
    // View
    TogglePreview,
    ToggleRulers,
    ToggleTimeline,
    ToggleChecks,
    ClearGuides,
    CycleTiling,
    // File
    ExportImage,
    BatchExport,
    ImportAnimation,
    ImportPalette,
    ExportPalette,
    RunScript,
    OpenScriptsFolder,
}

const fn key(k: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::NONE, k)
}
const fn cmd(k: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND, k)
}
const fn cmd_shift(k: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), k)
}
const fn shift(k: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::SHIFT, k)
}
const fn alt(k: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::ALT, k)
}

impl PixelCmd {
    pub const ALL: [PixelCmd; 73] = [
        Self::ScaleImage,
        Self::CropToSelection,
        Self::Trim,
        Self::RotateCanvasCw,
        Self::RotateCanvasCcw,
        Self::RotateCanvas180,
        Self::IndexedMode,
        Self::InvertSelection,
        Self::SelectLayerAlpha,
        Self::SaveSelection,
        Self::CopyMerged,
        Self::PasteInPlace,
        Self::FillSelection,
        Self::NudgeLeft,
        Self::NudgeRight,
        Self::NudgeUp,
        Self::NudgeDown,
        Self::NudgeLeftFar,
        Self::NudgeRightFar,
        Self::NudgeUpFar,
        Self::NudgeDownFar,
        Self::Commit,
        Self::FlipSelectionH,
        Self::FlipSelectionV,
        Self::RotateCw,
        Self::RotateCcw,
        Self::Rotate180,
        Self::RotateBy,
        Self::ScaleSelection,
        Self::NewGroup,
        Self::GroupLayer,
        Self::Ungroup,
        Self::MergeVisible,
        Self::FlattenLayers,
        Self::LayerViaCopy,
        Self::LayerViaCut,
        Self::AddReference,
        Self::ToggleLayerLock,
        Self::ToggleAlphaLock,
        Self::NewTilemap,
        Self::NewFrame,
        Self::DuplicateFrame,
        Self::NewLinkedFrame,
        Self::DeleteFrame,
        Self::PrevFrame,
        Self::NextFrame,
        Self::FirstFrame,
        Self::LastFrame,
        Self::PlayAnimation,
        Self::ToggleOnionSkin,
        Self::AddTag,
        Self::LinkCels,
        Self::UnlinkCel,
        Self::Filters,
        Self::Outline,
        Self::ReplaceColor,
        Self::PaletteSwap,
        Self::ReduceColors,
        Self::Invert,
        Self::Desaturate,
        Self::TogglePreview,
        Self::ToggleRulers,
        Self::ToggleTimeline,
        Self::ToggleChecks,
        Self::ClearGuides,
        Self::CycleTiling,
        Self::ExportImage,
        Self::BatchExport,
        Self::ImportAnimation,
        Self::ImportPalette,
        Self::ExportPalette,
        Self::RunScript,
        Self::OpenScriptsFolder,
    ];

    /// Every command once, in menu order.
    pub fn all() -> impl Iterator<Item = Self> {
        Self::ALL.into_iter()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ScaleImage => "Scale Image…",
            Self::CropToSelection => "Crop to Selection",
            Self::Trim => "Trim Transparent Edges",
            Self::RotateCanvasCw => "Rotate Canvas 90° Clockwise",
            Self::RotateCanvasCcw => "Rotate Canvas 90° Counter-clockwise",
            Self::RotateCanvas180 => "Rotate Canvas 180°",
            Self::IndexedMode => "Indexed Colour Mode",
            Self::InvertSelection => "Invert Selection",
            Self::SelectLayerAlpha => "Select Layer Pixels",
            Self::SaveSelection => "Save Selection…",
            Self::CopyMerged => "Copy Merged",
            Self::PasteInPlace => "Paste Into Layer",
            Self::FillSelection => "Fill Selection",
            Self::NudgeLeft => "Nudge Left",
            Self::NudgeRight => "Nudge Right",
            Self::NudgeUp => "Nudge Up",
            Self::NudgeDown => "Nudge Down",
            Self::NudgeLeftFar => "Nudge Left 10 px",
            Self::NudgeRightFar => "Nudge Right 10 px",
            Self::NudgeUpFar => "Nudge Up 10 px",
            Self::NudgeDownFar => "Nudge Down 10 px",
            Self::Commit => "Apply Moved Pixels",
            Self::FlipSelectionH => "Flip Selection Horizontally",
            Self::FlipSelectionV => "Flip Selection Vertically",
            Self::RotateCw => "Rotate Selection 90° Clockwise",
            Self::RotateCcw => "Rotate Selection 90° Counter-clockwise",
            Self::Rotate180 => "Rotate Selection 180°",
            Self::RotateBy => "Rotate Selection…",
            Self::ScaleSelection => "Scale Selection…",
            Self::NewGroup => "New Group",
            Self::GroupLayer => "Group Layer",
            Self::Ungroup => "Ungroup",
            Self::MergeVisible => "Merge Visible Layers",
            Self::FlattenLayers => "Flatten Image",
            Self::LayerViaCopy => "New Layer from Selection",
            Self::LayerViaCut => "Cut Selection to New Layer",
            Self::AddReference => "Add Reference Image…",
            Self::ToggleLayerLock => "Lock Layer",
            Self::ToggleAlphaLock => "Lock Transparency",
            Self::NewTilemap => "New Tilemap Layer…",
            Self::NewFrame => "New Frame",
            Self::DuplicateFrame => "Duplicate Frame",
            Self::NewLinkedFrame => "New Linked Frame",
            Self::DeleteFrame => "Delete Frame",
            Self::PrevFrame => "Previous Frame",
            Self::NextFrame => "Next Frame",
            Self::FirstFrame => "First Frame",
            Self::LastFrame => "Last Frame",
            Self::PlayAnimation => "Play / Pause",
            Self::ToggleOnionSkin => "Onion Skin",
            Self::AddTag => "Tag Frames…",
            Self::LinkCels => "Link Cels",
            Self::UnlinkCel => "Unlink Cel",
            Self::Filters => "Adjustments and Effects…",
            Self::Outline => "Outline…",
            Self::ReplaceColor => "Replace Colour…",
            Self::PaletteSwap => "Palette Swap…",
            Self::ReduceColors => "Reduce Colours and Dither…",
            Self::Invert => "Invert Colours",
            Self::Desaturate => "Desaturate",
            Self::TogglePreview => "Preview Window",
            Self::ToggleRulers => "Rulers and Guides",
            Self::ToggleTimeline => "Timeline",
            Self::ToggleChecks => "Show Problems",
            Self::ClearGuides => "Clear Guides",
            Self::CycleTiling => "Cycle Tiling Mode",
            Self::ExportImage => "Export Image or Animation…",
            Self::BatchExport => "Batch Export Textures…",
            Self::ImportAnimation => "Import Animation or Frames…",
            Self::ImportPalette => "Import Palette…",
            Self::ExportPalette => "Export Palette…",
            Self::RunScript => "Run Script…",
            Self::OpenScriptsFolder => "Open Scripts Folder",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::ScaleImage | Self::ScaleSelection => icons::RESIZE,
            Self::CropToSelection | Self::Trim => icons::CROP,
            Self::RotateCanvasCw | Self::RotateCw => icons::REDO,
            Self::RotateCanvasCcw | Self::RotateCcw => icons::UNDO,
            Self::RotateCanvas180 | Self::Rotate180 | Self::RotateBy => icons::REFRESH,
            Self::IndexedMode => icons::PALETTE,
            Self::InvertSelection | Self::SelectLayerAlpha | Self::SaveSelection => icons::SELECT,
            Self::CopyMerged => icons::COPY,
            Self::PasteInPlace => icons::CLIPBOARD,
            Self::FillSelection => icons::FILL,
            Self::NudgeLeft
            | Self::NudgeRight
            | Self::NudgeUp
            | Self::NudgeDown
            | Self::NudgeLeftFar
            | Self::NudgeRightFar
            | Self::NudgeUpFar
            | Self::NudgeDownFar => icons::MOVE,
            Self::Commit => icons::CHECK,
            Self::FlipSelectionH => icons::FLIP_H,
            Self::FlipSelectionV => icons::FLIP_V,
            Self::NewGroup | Self::GroupLayer | Self::Ungroup => icons::FOLDER,
            Self::MergeVisible | Self::FlattenLayers => icons::MERGE_DOWN,
            Self::LayerViaCopy | Self::LayerViaCut => icons::LAYERS,
            Self::AddReference => icons::IMAGES,
            Self::ToggleLayerLock | Self::ToggleAlphaLock => icons::LOCK,
            Self::NewTilemap => icons::STAMP,
            Self::NewFrame | Self::DuplicateFrame | Self::NewLinkedFrame => icons::PLUS,
            Self::DeleteFrame => icons::TRASH,
            Self::PrevFrame | Self::FirstFrame => icons::SKIP_BACK,
            Self::NextFrame | Self::LastFrame => icons::SKIP_FORWARD,
            Self::PlayAnimation => icons::PLAY,
            Self::ToggleOnionSkin => icons::GHOST,
            Self::AddTag => icons::TAG,
            Self::LinkCels => icons::LINK,
            Self::UnlinkCel => icons::UNLINK,
            Self::Filters | Self::ReplaceColor => icons::SLIDERS,
            Self::Outline => icons::SQUARE,
            Self::PaletteSwap | Self::ReduceColors => icons::PALETTE,
            Self::Invert | Self::Desaturate => icons::CONTRAST,
            Self::TogglePreview => icons::IMAGES,
            Self::ToggleRulers | Self::ClearGuides => icons::RULER,
            Self::ToggleTimeline => icons::ANIMATION,
            Self::ToggleChecks => icons::WARNING_CIRCLE,
            Self::CycleTiling => icons::TILING,
            Self::ExportImage | Self::BatchExport | Self::ExportPalette => icons::EXPORT,
            Self::ImportAnimation | Self::ImportPalette => icons::DOWNLOAD,
            Self::RunScript | Self::OpenScriptsFolder => icons::SCRIPT,
        }
    }

    pub fn default_shortcuts(self) -> &'static [KeyboardShortcut] {
        macro_rules! keys {
            ($($s:expr),*) => {{
                const KEYS: &[KeyboardShortcut] = &[$($s),*];
                KEYS
            }};
        }
        match self {
            Self::InvertSelection => keys![cmd_shift(Key::I)],
            Self::FillSelection => keys![alt(Key::Backspace)],
            Self::NudgeLeft => keys![key(Key::ArrowLeft)],
            Self::NudgeRight => keys![key(Key::ArrowRight)],
            Self::NudgeUp => keys![key(Key::ArrowUp)],
            Self::NudgeDown => keys![key(Key::ArrowDown)],
            Self::NudgeLeftFar => keys![shift(Key::ArrowLeft)],
            Self::NudgeRightFar => keys![shift(Key::ArrowRight)],
            Self::NudgeUpFar => keys![shift(Key::ArrowUp)],
            Self::NudgeDownFar => keys![shift(Key::ArrowDown)],
            Self::PlayAnimation => keys![key(Key::Enter)],
            Self::FlipSelectionH => keys![shift(Key::H)],
            Self::FlipSelectionV => keys![shift(Key::V)],
            Self::RotateCw => keys![shift(Key::R)],
            Self::RotateCcw => keys![shift(Key::E)],
            Self::GroupLayer => keys![cmd_shift(Key::G)],
            Self::LayerViaCopy => keys![cmd(Key::J)],
            Self::LayerViaCut => keys![cmd_shift(Key::J)],
            Self::NewFrame => keys![alt(Key::N)],
            Self::DuplicateFrame => keys![alt(Key::D)],
            Self::DeleteFrame => keys![alt(Key::Delete)],
            Self::PrevFrame => keys![key(Key::Comma)],
            Self::NextFrame => keys![key(Key::Period)],
            Self::FirstFrame => keys![key(Key::Home)],
            Self::LastFrame => keys![key(Key::End)],
            Self::ToggleOnionSkin => keys![key(Key::F3)],
            Self::TogglePreview => keys![key(Key::F4)],
            Self::ToggleRulers => keys![cmd(Key::R)],
            Self::ToggleTimeline => keys![key(Key::F6)],
            Self::ToggleChecks => keys![key(Key::F7)],
            Self::ExportImage => keys![cmd_shift(Key::E)],
            Self::Filters => keys![cmd(Key::U)],
            _ => &[],
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            Self::ScaleImage
            | Self::CropToSelection
            | Self::Trim
            | Self::RotateCanvasCw
            | Self::RotateCanvasCcw
            | Self::RotateCanvas180
            | Self::IndexedMode => "Image",
            Self::InvertSelection
            | Self::SelectLayerAlpha
            | Self::SaveSelection
            | Self::CopyMerged
            | Self::PasteInPlace
            | Self::FillSelection
            | Self::NudgeLeft
            | Self::NudgeRight
            | Self::NudgeUp
            | Self::NudgeDown
            | Self::NudgeLeftFar
            | Self::NudgeRightFar
            | Self::NudgeUpFar
            | Self::NudgeDownFar
            | Self::Commit
            | Self::FlipSelectionH
            | Self::FlipSelectionV
            | Self::RotateCw
            | Self::RotateCcw
            | Self::Rotate180
            | Self::RotateBy
            | Self::ScaleSelection => "Select",
            Self::NewGroup
            | Self::GroupLayer
            | Self::Ungroup
            | Self::MergeVisible
            | Self::FlattenLayers
            | Self::LayerViaCopy
            | Self::LayerViaCut
            | Self::AddReference
            | Self::ToggleLayerLock
            | Self::ToggleAlphaLock
            | Self::NewTilemap => "Layer",
            Self::NewFrame
            | Self::DuplicateFrame
            | Self::NewLinkedFrame
            | Self::DeleteFrame
            | Self::PrevFrame
            | Self::NextFrame
            | Self::FirstFrame
            | Self::LastFrame
            | Self::PlayAnimation
            | Self::ToggleOnionSkin
            | Self::AddTag
            | Self::LinkCels
            | Self::UnlinkCel => "Frame",
            Self::Filters
            | Self::Outline
            | Self::ReplaceColor
            | Self::PaletteSwap
            | Self::ReduceColors
            | Self::Invert
            | Self::Desaturate => "Filter",
            Self::TogglePreview
            | Self::ToggleRulers
            | Self::ToggleTimeline
            | Self::ToggleChecks
            | Self::ClearGuides
            | Self::CycleTiling => "View",
            Self::ExportImage
            | Self::BatchExport
            | Self::ImportAnimation
            | Self::ImportPalette
            | Self::ExportPalette
            | Self::RunScript
            | Self::OpenScriptsFolder => "File",
        }
    }

    /// Commands that need no open texture.
    pub fn needs_pack_only(self) -> bool {
        matches!(
            self,
            Self::BatchExport
                | Self::ImportAnimation
                | Self::ImportPalette
                | Self::OpenScriptsFolder
        )
    }

    /// Hidden from menus (still in the palette and the shortcut sheet).
    pub fn hidden(self) -> bool {
        matches!(
            self,
            Self::NudgeLeft
                | Self::NudgeRight
                | Self::NudgeUp
                | Self::NudgeDown
                | Self::NudgeLeftFar
                | Self::NudgeRightFar
                | Self::NudgeUpFar
                | Self::NudgeDownFar
                | Self::Commit
        )
    }

    /// Commands of a menu category in menu order.
    pub fn in_category(category: &str) -> Vec<PixelCmd> {
        Self::all()
            .filter(|c| c.category() == category && !c.hidden())
            .collect()
    }
}
