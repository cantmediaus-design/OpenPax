//! Every user action that has a menu entry, toolbar button or keyboard shortcut.
//!
//! Menus, toolbars and shortcut handling all read labels, icons and keys from here, so they
//! cannot drift apart. Shortcuts can be rebound; [`set_overrides`] installs the user's choices.

use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, RwLock};

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use openpax_paint::ToolKind;

use crate::icons;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    // File
    NewPack,
    OpenPack,
    ImportZip,
    LoadDefaultPack,
    Remix,
    NewTexture,
    NewModel,
    Save,
    SaveAll,
    CloseTab,
    ExportZip,
    Rescan,
    RenameAsset,
    DuplicateAsset,
    DeleteAsset,
    RestoreDeleted,
    Quit,
    // Edit
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
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
    OpenPalette,
    // Help
    ShowShortcuts,
    StartTour,
    About,
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
    const FIXED: [Self; 47] = [
        Self::NewPack,
        Self::OpenPack,
        Self::ImportZip,
        Self::LoadDefaultPack,
        Self::Remix,
        Self::NewTexture,
        Self::NewModel,
        Self::Save,
        Self::SaveAll,
        Self::CloseTab,
        Self::ExportZip,
        Self::Rescan,
        Self::RenameAsset,
        Self::DuplicateAsset,
        Self::DeleteAsset,
        Self::RestoreDeleted,
        Self::Quit,
        Self::Undo,
        Self::Redo,
        Self::Cut,
        Self::Copy,
        Self::Paste,
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
        Self::OpenPalette,
        Self::ShowShortcuts,
        Self::StartTour,
        Self::About,
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
            Self::LoadDefaultPack => "Load Default Pack…",
            Self::Remix => "Remix Packs…",
            Self::NewTexture => "New Texture…",
            Self::NewModel => "New Model…",
            Self::Save => "Save",
            Self::SaveAll => "Save All",
            Self::CloseTab => "Close Tab",
            Self::ExportZip => "Export Zip…",
            Self::Rescan => "Rescan Pack Folder",
            Self::RenameAsset => "Rename or Move File…",
            Self::DuplicateAsset => "Duplicate File",
            Self::DeleteAsset => "Delete File…",
            Self::RestoreDeleted => "Restore Deleted File",
            Self::Quit => "Quit",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste as New Layer",
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
            Self::OpenPalette => "Command Palette…",
            Self::ShowShortcuts => "Keyboard Shortcuts",
            Self::StartTour => "Show the Tour",
            Self::About => "About OpenPax",
            Self::Tool(tool) => tool.name(),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::NewPack => icons::FOLDER_PLUS,
            Self::OpenPack => icons::FOLDER_OPEN,
            Self::ImportZip => icons::FILE_ZIP,
            Self::LoadDefaultPack => icons::PACKAGE,
            Self::Remix => icons::MERGE_DOWN,
            Self::NewTexture => icons::IMAGE,
            Self::NewModel => icons::CUBE,
            Self::Save | Self::SaveAll => icons::SAVE,
            Self::CloseTab => icons::CLOSE,
            Self::ExportZip => icons::EXPORT,
            Self::Rescan => icons::REFRESH,
            Self::RenameAsset => icons::RENAME,
            Self::DuplicateAsset => icons::COPY,
            Self::DeleteAsset => icons::TRASH,
            Self::RestoreDeleted => icons::UNDO,
            Self::Quit => icons::QUIT,
            Self::Undo => icons::UNDO,
            Self::Redo => icons::REDO,
            Self::Cut => icons::SCISSORS,
            Self::Copy => icons::COPY,
            Self::Paste => icons::CLIPBOARD,
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
            Self::OpenPalette => icons::COMMAND,
            Self::ShowShortcuts => icons::KEYBOARD,
            Self::StartTour => icons::LIGHTBULB,
            Self::About => icons::INFO,
            Self::Tool(tool) => icons::tool(tool),
        }
    }

    /// Keyboard shortcuts, as rebound by the user; the first one is the one shown in menus and
    /// tooltips.
    pub fn shortcuts(self) -> Vec<KeyboardShortcut> {
        OVERRIDES
            .read()
            .ok()
            .and_then(|map| map.get(&self).cloned())
            .unwrap_or_else(|| self.default_shortcuts().to_vec())
    }

    /// Whether the user changed this command's shortcuts.
    pub fn is_rebound(self) -> bool {
        OVERRIDES.read().is_ok_and(|map| map.contains_key(&self))
    }

    /// Whether the shortcut can be changed. Cut, copy and paste arrive as clipboard events
    /// from the system, not as key presses, so they keep their standard keys.
    pub fn rebindable(self) -> bool {
        !matches!(self, Self::Cut | Self::Copy | Self::Paste)
    }

    /// A stable name for storing the command in settings.
    pub fn id(self) -> String {
        format!("{self:?}")
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::all().find(|c| c.id() == id)
    }

    /// The built-in keyboard shortcuts.
    pub fn default_shortcuts(self) -> &'static [KeyboardShortcut] {
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
            Self::RenameAsset => keys![key(Key::F2)],
            Self::Undo => keys![cmd(Key::Z)],
            Self::Redo => keys![cmd_shift(Key::Z), cmd(Key::Y)],
            // egui turns these into clipboard events rather than key presses; listed so menus
            // and the shortcut sheet show them.
            Self::Cut => keys![cmd(Key::X)],
            Self::Copy => keys![cmd(Key::C)],
            Self::Paste => keys![cmd(Key::V)],
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
            Self::OpenPalette => keys![cmd_shift(Key::P), cmd(Key::K)],
            Self::ShowShortcuts => keys![key(Key::F1), cmd(Key::Slash)],
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
            | Self::LoadDefaultPack
            | Self::Remix
            | Self::NewModel
            | Self::Quit
            | Self::FlipHorizontal
            | Self::FlipVertical
            | Self::ResizeCanvas
            | Self::AddCube
            | Self::ToggleTiling
            | Self::DuplicateAsset
            | Self::DeleteAsset
            | Self::RestoreDeleted
            | Self::StartTour
            | Self::About => &[],
        }
    }

    /// The menu this command belongs to, for grouping in the shortcut sheet and the palette.
    pub fn category(self) -> &'static str {
        match self {
            Self::NewPack
            | Self::OpenPack
            | Self::ImportZip
            | Self::LoadDefaultPack
            | Self::Remix
            | Self::NewTexture
            | Self::NewModel
            | Self::Save
            | Self::SaveAll
            | Self::CloseTab
            | Self::ExportZip
            | Self::Rescan
            | Self::RenameAsset
            | Self::DuplicateAsset
            | Self::DeleteAsset
            | Self::RestoreDeleted
            | Self::Quit => "File",
            Self::Undo
            | Self::Redo
            | Self::Cut
            | Self::Copy
            | Self::Paste
            | Self::SelectAll
            | Self::Deselect
            | Self::ClearSelection
            | Self::FlipHorizontal
            | Self::FlipVertical
            | Self::ResizeCanvas
            | Self::SwapColors
            | Self::BrushSmaller
            | Self::BrushLarger
            | Self::AddCube
            | Self::DuplicateElement
            | Self::DeleteElement => "Edit",
            Self::ZoomIn
            | Self::ZoomOut
            | Self::ZoomFit
            | Self::ToggleGrid
            | Self::ToggleTiling
            | Self::ResetView
            | Self::ToggleExplorer
            | Self::ToggleInspector
            | Self::FocusSearch
            | Self::OpenPalette => "View",
            Self::Tool(_) => "Tools",
            Self::ShowShortcuts | Self::StartTour | Self::About => "Help",
        }
    }

    /// Where the command applies, for the shortcut sheet.
    pub fn context(self) -> &'static str {
        match self.needs() {
            Needs::Nothing | Needs::Pack | Needs::Tab | Needs::History => "",
            Needs::Texture => "Texture",
            Needs::Model => "Model",
            Needs::TextureOrModel => "Texture, model",
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
            | Self::LoadDefaultPack
            | Self::Remix
            | Self::Quit
            | Self::ToggleExplorer
            | Self::ToggleInspector
            | Self::OpenPalette
            | Self::ShowShortcuts
            | Self::About => Needs::Nothing,
            Self::NewTexture
            | Self::NewModel
            | Self::SaveAll
            | Self::ExportZip
            | Self::Rescan
            | Self::RestoreDeleted
            | Self::StartTour
            | Self::FocusSearch => Needs::Pack,
            Self::Save
            | Self::CloseTab
            | Self::RenameAsset
            | Self::DuplicateAsset
            | Self::DeleteAsset => Needs::Tab,
            Self::Undo | Self::Redo => Needs::History,
            Self::Cut
            | Self::Copy
            | Self::Paste
            | Self::SelectAll
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
                | Self::OpenPalette
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
        .flat_map(|c| c.shortcuts().into_iter().map(move |s| (c, s)))
        .collect();
    bindings.sort_by_key(|(_, s)| std::cmp::Reverse(weight(s.modifiers)));
    bindings
}

/// Shortcuts the user has rebound, replacing the defaults of those commands.
static OVERRIDES: LazyLock<RwLock<HashMap<Command, Vec<KeyboardShortcut>>>> =
    LazyLock::new(Default::default);

/// Every combination of open pack and editor kind, for telling whether two commands can
/// both be available at once.
const STATES: [(bool, Active); 5] = [
    (false, Active::None),
    (true, Active::None),
    (true, Active::Texture),
    (true, Active::Model),
    (true, Active::Text),
];

/// Whether two commands can ever be available at the same time.
fn overlap(a: Command, b: Command) -> bool {
    STATES
        .iter()
        .any(|(pack, active)| a.needs().met(*pack, *active) && b.needs().met(*pack, *active))
}

/// Other commands that already use `shortcut` where `command` would.
pub fn conflicts(command: Command, shortcut: KeyboardShortcut) -> Vec<Command> {
    Command::all()
        .filter(|c| *c != command && overlap(*c, command) && c.shortcuts().contains(&shortcut))
        .collect()
}

/// Installs rebound shortcuts read from settings (command id to shortcut strings). Unknown
/// commands and keys are skipped.
pub fn set_overrides(stored: &BTreeMap<String, Vec<String>>) {
    let map = stored
        .iter()
        .filter_map(|(id, keys)| {
            let command = Command::from_id(id).filter(|c| c.rebindable())?;
            Some((
                command,
                keys.iter().filter_map(|k| parse_shortcut(k)).collect(),
            ))
        })
        .collect();
    if let Ok(mut overrides) = OVERRIDES.write() {
        *overrides = map;
    }
}

/// Turns the current overrides back into the form stored in settings.
pub fn stored_overrides() -> BTreeMap<String, Vec<String>> {
    OVERRIDES
        .read()
        .map(|map| {
            map.iter()
                .map(|(c, keys)| (c.id(), keys.iter().map(shortcut_to_string).collect()))
                .collect()
        })
        .unwrap_or_default()
}

/// Gives `command` the shortcut `shortcut` (alone), taking it away from any command it
/// conflicts with. Returns the commands that lost it.
pub fn rebind(command: Command, shortcut: Option<KeyboardShortcut>) -> Vec<Command> {
    let taken = shortcut.map(|s| conflicts(command, s)).unwrap_or_default();
    if let Ok(mut overrides) = OVERRIDES.write() {
        for other in &taken {
            let keys: Vec<KeyboardShortcut> = overrides
                .get(other)
                .cloned()
                .unwrap_or_else(|| other.default_shortcuts().to_vec())
                .into_iter()
                .filter(|k| Some(*k) != shortcut)
                .collect();
            overrides.insert(*other, keys);
        }
        overrides.insert(command, shortcut.into_iter().collect());
    }
    taken
}

/// Puts one command (or, with `None`, every command) back to its default shortcuts.
pub fn reset(command: Option<Command>) {
    if let Ok(mut overrides) = OVERRIDES.write() {
        match command {
            Some(command) => drop(overrides.remove(&command)),
            None => overrides.clear(),
        }
    }
}

/// `Cmd+Shift+S` style text for settings; `Cmd` is Ctrl outside macOS.
pub fn shortcut_to_string(shortcut: &KeyboardShortcut) -> String {
    let m = shortcut.modifiers;
    let mut parts = Vec::new();
    if m.command || m.ctrl || m.mac_cmd {
        parts.push("Cmd");
    }
    if m.alt {
        parts.push("Alt");
    }
    if m.shift {
        parts.push("Shift");
    }
    parts.push(shortcut.logical_key.name());
    parts.join("+")
}

pub fn parse_shortcut(text: &str) -> Option<KeyboardShortcut> {
    let mut parts: Vec<&str> = text.split('+').collect();
    // "Plus" is a key name, but a bare "+" would split into empty parts.
    let key = Key::from_name(parts.pop()?)?;
    let mut modifiers = Modifiers::NONE;
    for part in parts {
        modifiers = match part {
            "Cmd" | "Ctrl" => modifiers.plus(Modifiers::COMMAND),
            "Alt" => modifiers.plus(Modifiers::ALT),
            "Shift" => modifiers.plus(Modifiers::SHIFT),
            _ => return None,
        };
    }
    Some(KeyboardShortcut::new(modifiers, key))
}

/// The shortcut for a key pressed with `modifiers`, with Ctrl / Cmd folded into "command"
/// so the same binding works on every platform.
pub fn shortcut_for(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
    let mut normalized = Modifiers::NONE;
    if modifiers.command || modifiers.ctrl || modifiers.mac_cmd {
        normalized = normalized.plus(Modifiers::COMMAND);
    }
    if modifiers.alt {
        normalized = normalized.plus(Modifiers::ALT);
    }
    if modifiers.shift {
        normalized = normalized.plus(Modifiers::SHIFT);
    }
    KeyboardShortcut::new(normalized, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests read the defaults only: overrides are process-wide, and tests run in
    // parallel.

    #[test]
    fn shortcuts_survive_being_stored_as_text() {
        for command in Command::all() {
            for shortcut in command.default_shortcuts() {
                let text = shortcut_to_string(shortcut);
                assert_eq!(parse_shortcut(&text), Some(*shortcut), "{text}");
            }
        }
        assert_eq!(shortcut_to_string(&cmd_shift(Key::S)), "Cmd+Shift+S");
        assert_eq!(parse_shortcut("Ctrl+Nonsense"), None);
        assert_eq!(parse_shortcut("Hyper+S"), None);
    }

    #[test]
    fn conflicts_only_count_where_both_commands_apply() {
        // Ctrl+G is the grid toggle; G alone is the fill tool.
        assert_eq!(
            conflicts(Command::Tool(ToolKind::Fill), cmd(Key::G)),
            [Command::ToggleGrid]
        );
        // Delete clears a texture selection and deletes a model element, never both at once.
        assert!(conflicts(Command::ClearSelection, key(Key::Delete)).is_empty());
        assert!(conflicts(Command::Save, cmd(Key::S)).is_empty());
    }

    #[test]
    fn ctrl_and_cmd_both_become_command() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        assert_eq!(shortcut_for(ctrl, Key::S), cmd(Key::S));
        assert_eq!(shortcut_for(Modifiers::MAC_CMD, Key::S), cmd(Key::S));
        assert_eq!(shortcut_for(Modifiers::NONE, Key::B), key(Key::B));
    }

    #[test]
    fn commands_round_trip_through_their_ids() {
        for command in Command::all() {
            assert_eq!(Command::from_id(&command.id()), Some(command));
        }
    }

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
