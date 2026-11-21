//! Every user action that has a menu entry, toolbar button or keyboard shortcut.
//!
//! Menus, toolbars and shortcut handling all read labels, icons and keys from here, so they
//! cannot drift apart.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use openpax_paint::ToolKind;

use crate::icons;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    // File
    NewPack,
    OpenPack,
    ImportZip,
    NewTexture,
    NewModel,
    Save,
    SaveAll,
    CloseTab,
    ExportZip,
    Rescan,
    Quit,
    // Edit
    Undo,
    Redo,
    SelectAll,
    Deselect,
    ClearSelection,
    FlipHorizontal,
    FlipVertical,
    ResizeCanvas,
    SwapColors,
    BrushSmaller,
    BrushLarger,
    AddCube,
    DuplicateElement,
    DeleteElement,
    // View
    ZoomIn,
    ZoomOut,
    ZoomFit,
    ToggleGrid,
    ToggleTiling,
    ResetView,
    ToggleExplorer,
    ToggleInspector,
    FocusSearch,
    Tool(ToolKind),
}

/// What must be open for a command to make sense.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs {
    Nothing,
    Pack,
    /// Any editor tab.
    Tab,
    /// A tab with its own undo history (texture or model).
    History,
    Texture,
    Model,
    TextureOrModel,
}

/// The kind of editor in the focused tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Active {
    None,
    Texture,
    Model,
    Text,
}

impl Needs {
    pub fn met(self, has_pack: bool, active: Active) -> bool {
        match self {
            Self::Nothing => true,
            Self::Pack => has_pack,
            Self::Tab => active != Active::None,
            Self::History | Self::TextureOrModel => {
                matches!(active, Active::Texture | Active::Model)
            }
            Self::Texture => active == Active::Texture,
            Self::Model => active == Active::Model,
        }
    }
}

const fn key(logical_key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::NONE, logical_key)
}

const fn cmd(logical_key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND, logical_key)
}

const fn cmd_shift(logical_key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), logical_key)
}

impl Command {
    const FIXED: [Self; 34] = [
        Self::NewPack,
        Self::OpenPack,
        Self::ImportZip,
        Self::NewTexture,
        Self::NewModel,
        Self::Save,
        Self::SaveAll,
        Self::CloseTab,
        Self::ExportZip,
        Self::Rescan,
        Self::Quit,
        Self::Undo,
        Self::Redo,
        Self::SelectAll,
        Self::Deselect,
        Self::ClearSelection,
        Self::FlipHorizontal,
        Self::FlipVertical,
        Self::ResizeCanvas,
        Self::SwapColors,
        Self::BrushSmaller,
        Self::BrushLarger,
        Self::AddCube,
        Self::DuplicateElement,
        Self::DeleteElement,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::ZoomFit,
        Self::ToggleGrid,
        Self::ToggleTiling,
        Self::ResetView,
        Self::ToggleExplorer,
        Self::ToggleInspector,
        Self::FocusSearch,
    ];

    pub fn all() -> impl Iterator<Item = Self> {
        Self::FIXED
            .into_iter()
            .chain(ToolKind::ALL.into_iter().map(Self::Tool))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NewPack => "New Pack…",
            Self::OpenPack => "Open Pack Folder…",
            Self::ImportZip => "Import Pack Zip…",
            Self::NewTexture => "New Texture…",
            Self::NewModel => "New Model…",
            Self::Save => "Save",
            Self::SaveAll => "Save All",
            Self::CloseTab => "Close Tab",
            Self::ExportZip => "Export Zip…",
            Self::Rescan => "Rescan Pack Folder",
            Self::Quit => "Quit",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::SelectAll => "Select All",
            Self::Deselect => "Deselect",
            Self::ClearSelection => "Clear Selection",
            Self::FlipHorizontal => "Flip Horizontally",
            Self::FlipVertical => "Flip Vertically",
            Self::ResizeCanvas => "Resize Canvas…",
            Self::SwapColors => "Swap Colours",
            Self::BrushSmaller => "Smaller Brush",
            Self::BrushLarger => "Larger Brush",
            Self::AddCube => "Add Cube",
            Self::DuplicateElement => "Duplicate Element",
            Self::DeleteElement => "Delete Element",
            Self::ZoomIn => "Zoom In",
            Self::ZoomOut => "Zoom Out",
            Self::ZoomFit => "Zoom to Fit",
            Self::ToggleGrid => "Grid",
            Self::ToggleTiling => "Tiling Preview",
            Self::ResetView => "Reset View",
            Self::ToggleExplorer => "Explorer",
            Self::ToggleInspector => "Inspector",
            Self::FocusSearch => "Search Assets",
            Self::Tool(tool) => tool.name(),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::NewPack => icons::FOLDER_PLUS,
            Self::OpenPack => icons::FOLDER_OPEN,
            Self::ImportZip => icons::FILE_ZIP,
            Self::NewTexture => icons::IMAGE,
            Self::NewModel => icons::CUBE,
            Self::Save | Self::SaveAll => icons::SAVE,
            Self::CloseTab => icons::CLOSE,
            Self::ExportZip => icons::EXPORT,
            Self::Rescan => icons::REFRESH,
            Self::Quit => icons::QUIT,
            Self::Undo => icons::UNDO,
            Self::Redo => icons::REDO,
            Self::SelectAll | Self::Deselect => icons::SELECT,
            Self::ClearSelection => icons::ERASER,
            Self::FlipHorizontal => icons::FLIP_H,
            Self::FlipVertical => icons::FLIP_V,
            Self::ResizeCanvas => icons::RESIZE,
            Self::SwapColors => icons::SWAP,
            Self::BrushSmaller => icons::MINUS,
            Self::BrushLarger | Self::AddCube => icons::PLUS,
            Self::DuplicateElement => icons::COPY,
            Self::DeleteElement => icons::TRASH,
            Self::ZoomIn => icons::ZOOM_IN,
            Self::ZoomOut => icons::ZOOM_OUT,
            Self::ZoomFit => icons::FIT,
            Self::ToggleGrid => icons::GRID,
            Self::ToggleTiling => icons::TILING,
            Self::ResetView => icons::HOME,
            Self::ToggleExplorer | Self::ToggleInspector => icons::SIDEBAR,
            Self::FocusSearch => icons::SEARCH,
            Self::Tool(tool) => icons::tool(tool),
        }
    }

    /// Keyboard shortcuts; the first one is the one shown in menus and tooltips.
    pub fn shortcuts(self) -> &'static [KeyboardShortcut] {
        /// A `'static` slice of shortcuts (function calls are not promoted to statics).
        macro_rules! keys {
            ($($shortcut:expr),*) => {{
                const KEYS: &[KeyboardShortcut] = &[$($shortcut),*];
                KEYS
            }};
        }
        match self {
            Self::NewPack => keys![cmd_shift(Key::N)],
            Self::OpenPack => keys![cmd(Key::O)],
            Self::NewTexture => keys![cmd(Key::N)],
            Self::Save => keys![cmd(Key::S)],
            Self::SaveAll => keys![cmd_shift(Key::S)],
            Self::CloseTab => keys![cmd(Key::W)],
            Self::ExportZip => keys![cmd(Key::E)],
            Self::Rescan => keys![key(Key::F5)],
            Self::Undo => keys![cmd(Key::Z)],
            Self::Redo => keys![cmd_shift(Key::Z), cmd(Key::Y)],
            Self::SelectAll => keys![cmd(Key::A)],
            Self::Deselect => keys![cmd(Key::D), key(Key::Escape)],
            Self::ClearSelection | Self::DeleteElement => keys![key(Key::Delete)],
            Self::SwapColors => keys![key(Key::X)],
            Self::BrushSmaller => keys![key(Key::OpenBracket)],
            Self::BrushLarger => keys![key(Key::CloseBracket)],
            Self::DuplicateElement => keys![cmd(Key::D)],
            Self::ZoomIn => keys![cmd(Key::Equals), cmd(Key::Plus)],
            Self::ZoomOut => keys![cmd(Key::Minus)],
            Self::ZoomFit | Self::ResetView => keys![cmd(Key::Num0)],
            Self::ToggleGrid => keys![cmd(Key::G)],
            Self::ToggleExplorer => keys![cmd(Key::B)],
            Self::ToggleInspector => keys![cmd(Key::I)],
            Self::FocusSearch => keys![cmd(Key::F)],
            Self::Tool(tool) => match tool {
                ToolKind::Pencil => keys![key(Key::B)],
                ToolKind::Eraser => keys![key(Key::E)],
                ToolKind::Fill => keys![key(Key::G)],
                ToolKind::Picker => keys![key(Key::I)],
                ToolKind::Line => keys![key(Key::L)],
                ToolKind::Rectangle => keys![key(Key::R)],
                ToolKind::Ellipse => keys![key(Key::O)],
                ToolKind::Select => keys![key(Key::M)],
                ToolKind::Move => keys![key(Key::V)],
            },
            Self::ImportZip
            | Self::NewModel
            | Self::Quit
            | Self::FlipHorizontal
            | Self::FlipVertical
            | Self::ResizeCanvas
            | Self::AddCube
            | Self::ToggleTiling => &[],
        }
    }

    pub fn shortcut(self) -> Option<KeyboardShortcut> {
        self.shortcuts().first().copied()
    }

    pub fn needs(self) -> Needs {
        match self {
            Self::NewPack
            | Self::OpenPack
            | Self::ImportZip
            | Self::Quit
            | Self::ToggleExplorer
            | Self::ToggleInspector => Needs::Nothing,
            Self::NewTexture
            | Self::NewModel
            | Self::SaveAll
            | Self::ExportZip
            | Self::Rescan
            | Self::FocusSearch => Needs::Pack,
            Self::Save | Self::CloseTab => Needs::Tab,
            Self::Undo | Self::Redo => Needs::History,
            Self::SelectAll
            | Self::Deselect
            | Self::ClearSelection
            | Self::FlipHorizontal
            | Self::FlipVertical
            | Self::ResizeCanvas
            | Self::SwapColors
            | Self::BrushSmaller
            | Self::BrushLarger
            | Self::ZoomIn
            | Self::ZoomOut
            | Self::ZoomFit
            | Self::ToggleTiling
            | Self::Tool(_) => Needs::Texture,
            Self::AddCube | Self::DuplicateElement | Self::DeleteElement | Self::ResetView => {
                Needs::Model
            }
            Self::ToggleGrid => Needs::TextureOrModel,
        }
    }

    /// Whether the shortcut also fires while a text field has keyboard focus. Everything else
    /// is left to the text field (typing letters, its own undo, select all, delete).
    pub fn works_while_typing(self) -> bool {
        matches!(
            self,
            Self::NewPack
                | Self::OpenPack
                | Self::NewTexture
                | Self::Save
                | Self::SaveAll
                | Self::CloseTab
                | Self::ExportZip
                | Self::FocusSearch
                | Self::ToggleExplorer
                | Self::ToggleInspector
        )
    }

    /// "Label (Shortcut)" for tooltips.
    pub fn tooltip(self, ctx: &eframe::egui::Context) -> String {
        let label = self.label().trim_end_matches('…');
        match self.shortcut() {
            Some(shortcut) => format!("{label}  ({})", ctx.format_shortcut(&shortcut)),
            None => label.to_owned(),
        }
    }
}

/// `(command, shortcut)` pairs ordered so that shortcuts with more modifiers are tried first;
/// egui ignores extra Shift/Alt when matching, so Ctrl+Shift+S must win over Ctrl+S.
pub fn bindings() -> Vec<(Command, KeyboardShortcut)> {
    let weight = |m: Modifiers| {
        [m.alt, m.ctrl, m.shift, m.mac_cmd, m.command]
            .iter()
            .filter(|b| **b)
            .count()
    };
    let mut bindings: Vec<(Command, KeyboardShortcut)> = Command::all()
        .flat_map(|c| c.shortcuts().iter().map(move |s| (c, *s)))
        .collect();
    bindings.sort_by_key(|(_, s)| std::cmp::Reverse(weight(s.modifiers)));
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATES: [(bool, Active); 5] = [
        (false, Active::None),
        (true, Active::None),
        (true, Active::Texture),
        (true, Active::Model),
        (true, Active::Text),
    ];

    #[test]
    fn no_two_commands_share_a_shortcut_in_the_same_context() {
        let bindings = bindings();
        for (i, (a, sa)) in bindings.iter().enumerate() {
            for (b, sb) in &bindings[i + 1..] {
                if sa != sb || a == b {
                    continue;
                }
                for (has_pack, active) in STATES {
                    assert!(
                        !(a.needs().met(has_pack, active) && b.needs().met(has_pack, active)),
                        "{a:?} and {b:?} both use {sa:?} with {active:?} active"
                    );
                }
            }
        }
    }

    #[test]
    fn more_specific_shortcuts_are_tried_first() {
        let bindings = bindings();
        let position = |c: Command| bindings.iter().position(|(b, _)| *b == c).unwrap();
        assert!(position(Command::SaveAll) < position(Command::Save));
        assert!(position(Command::Redo) < position(Command::Undo));
        assert!(position(Command::NewPack) < position(Command::NewTexture));
    }

    #[test]
    fn every_command_is_listed_once_with_a_label() {
        let all: Vec<Command> = Command::all().collect();
        for (i, command) in all.iter().enumerate() {
            assert!(!command.label().is_empty());
            assert!(!command.icon().is_empty());
            assert!(!all[i + 1..].contains(command), "{command:?} listed twice");
        }
        assert_eq!(all.len(), Command::FIXED.len() + ToolKind::ALL.len());
    }

    #[test]
    fn bare_key_shortcuts_never_fire_while_typing() {
        for command in Command::all() {
            if command.works_while_typing() {
                for shortcut in command.shortcuts() {
                    assert!(shortcut.modifiers.command, "{command:?}");
                }
            }
        }
    }
}
