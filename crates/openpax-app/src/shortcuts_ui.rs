//! The Keyboard Shortcuts sheet, which doubles as the place to rebind them: click a shortcut,
//! press the new keys, done. A key that another command was using is taken from it.

use eframe::egui::{self, Event, Key, Modifiers, RichText};

use crate::app::App;
use crate::commands::{self, Command};
use crate::dialogs;
use crate::theme::{SPACE_L, SPACE_M, SPACE_S};
use crate::{icons, widgets};

/// Mouse gestures that have no command of their own, for the shortcut sheet.
const MOUSE_HINTS: &[(&str, &str, &str)] = &[
    (
        "Paint with the primary / secondary colour",
        "Left / right button",
        "Texture",
    ),
    ("Pick a colour from the canvas", "Hold Alt", "Texture"),
    ("Pan", "Middle-drag or Space+drag", "Texture"),
    ("Zoom towards the cursor", "Scroll or pinch", "Texture"),
    ("Orbit / pan the camera", "Left-drag / right-drag", "Model"),
    ("Select an element", "Click it", "Model"),
];

#[derive(Default)]
pub struct ShortcutEditor {
    /// The command waiting for its new keys.
    recording: Option<Command>,
    /// What the last change did, e.g. which command lost its shortcut.
    notice: Option<String>,
    search: String,
}

/// The next key press, if any, taken out of this frame's input. Clipboard shortcuts arrive as
/// clipboard events and are turned back into their keys.
fn take_key_press(ui: &egui::Ui) -> Option<(Modifiers, Key)> {
    ui.input_mut(|input| {
        let mut found = None;
        input.events.retain(|event| {
            if found.is_some() {
                return true;
            }
            found = match event {
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*modifiers, *key)),
                Event::Copy => Some((Modifiers::COMMAND, Key::C)),
                Event::Cut => Some((Modifiers::COMMAND, Key::X)),
                Event::Paste(_) | Event::PasteImage(_) => Some((Modifiers::COMMAND, Key::V)),
                _ => return true,
            };
            false
        });
        found
    })
}

impl App {
    pub(crate) fn shortcuts_dialog(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ShortcutEditor,
    ) -> bool {
        dialogs::header(ui, icons::KEYBOARD, "Keyboard Shortcuts");
        let ctx = ui.ctx().clone();

        if let Some(command) = state.recording
            && let Some((modifiers, key)) = take_key_press(ui)
        {
            state.recording = None;
            state.notice = self.apply_key(command, modifiers, key, &ctx);
        }

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.search)
                    .hint_text(widgets::icon_label(icons::SEARCH, "Find a command…"))
                    .desired_width(240.0),
            );
            ui.weak("Click a shortcut to change it.");
        });
        if let Some(notice) = &state.notice {
            ui.add_space(SPACE_S);
            ui.label(RichText::new(notice).color(crate::theme::of(ui).accent));
        }
        ui.add_space(SPACE_S);

        let needle = state.search.trim().to_lowercase();
        egui::ScrollArea::vertical()
            .max_height(ctx.content_rect().height() * 0.55)
            .show(ui, |ui| {
                for category in ["File", "Edit", "View", "Tools", "Help"] {
                    let commands: Vec<Command> = Command::all()
                        .filter(|c| c.category() == category)
                        .filter(|c| needle.is_empty() || c.label().to_lowercase().contains(&needle))
                        .collect();
                    if commands.is_empty() {
                        continue;
                    }
                    widgets::caption(ui, &category.to_uppercase());
                    egui::Grid::new(("shortcuts", category))
                        .num_columns(4)
                        .spacing([SPACE_L, SPACE_S])
                        .striped(true)
                        .min_col_width(60.0)
                        .show(ui, |ui| {
                            for command in commands {
                                self.shortcut_row(ui, state, command);
                            }
                        });
                    ui.add_space(SPACE_M);
                }
                if needle.is_empty() {
                    widgets::caption(ui, "MOUSE");
                    egui::Grid::new("shortcuts_mouse")
                        .num_columns(3)
                        .spacing([SPACE_L, SPACE_S])
                        .striped(true)
                        .show(ui, |ui| {
                            for (action, gesture, context) in MOUSE_HINTS {
                                ui.label(widgets::icon_label(icons::INFO, action));
                                ui.label(RichText::new(*gesture).monospace().strong());
                                ui.weak(*context);
                                ui.end_row();
                            }
                        });
                }
            });

        ui.add_space(SPACE_S);
        let any_rebound = Command::all().any(Command::is_rebound);
        let mut close = false;
        ui.add_space(SPACE_L);
        egui::Sides::new().show(
            ui,
            |ui| {
                if ui
                    .add_enabled(any_rebound, egui::Button::new("Reset All to Defaults"))
                    .clicked()
                {
                    commands::reset(None);
                    self.settings.shortcuts.clear();
                    state.notice = Some("Every shortcut is back to its default.".to_owned());
                }
            },
            |ui| close = widgets::primary_button(ui, true, "Close").clicked(),
        );
        // While recording, Enter is a key to bind, not "close".
        let enter = state.recording.is_none() && ui.input(|i| i.key_pressed(Key::Enter));
        close || enter
    }

    fn shortcut_row(&mut self, ui: &mut egui::Ui, state: &mut ShortcutEditor, command: Command) {
        let ctx = ui.ctx().clone();
        ui.label(widgets::icon_label(
            command.icon(),
            command.label().trim_end_matches('…'),
        ));
        let keys: Vec<String> = command
            .shortcuts()
            .iter()
            .map(|s| ctx.format_shortcut(s))
            .collect();
        let shown = if keys.is_empty() {
            "—".to_owned()
        } else {
            keys.join("  or  ")
        };
        if state.recording == Some(command) {
            ui.label(
                RichText::new("Press keys…  (Esc cancels, Backspace clears)")
                    .color(crate::theme::of(ui).accent),
            );
        } else if command.rebindable() {
            let mut text = RichText::new(&shown).monospace().strong();
            if command.is_rebound() {
                text = text.color(crate::theme::of(ui).accent);
            }
            if ui
                .add(egui::Button::new(text).frame_when_inactive(false))
                .on_hover_text("Click, then press the new keys")
                .clicked()
            {
                state.recording = Some(command);
                state.notice = None;
            }
        } else {
            ui.label(RichText::new(&shown).monospace().strong())
                .on_hover_text("Clipboard shortcuts follow the system and cannot be changed");
        }
        ui.weak(command.context());
        if command.is_rebound() {
            if widgets::icon_button(ui, icons::UNDO, "Reset to the default").clicked() {
                commands::reset(Some(command));
                self.settings.shortcuts = commands::stored_overrides();
            }
        } else {
            ui.label("");
        }
        ui.end_row();
    }

    /// Binds a pressed key to `command`. Returns a note for the sheet.
    fn apply_key(
        &mut self,
        command: Command,
        modifiers: Modifiers,
        key: Key,
        ctx: &egui::Context,
    ) -> Option<String> {
        let bare = !(modifiers.command || modifiers.ctrl || modifiers.mac_cmd || modifiers.alt);
        if bare && key == Key::Escape {
            return None;
        }
        if bare && matches!(key, Key::Backspace | Key::Delete) && !modifiers.shift {
            commands::rebind(command, None);
            self.settings.shortcuts = commands::stored_overrides();
            return Some(format!("{} has no shortcut now.", command.label()));
        }
        let shortcut = commands::shortcut_for(modifiers, key);
        let taken = commands::rebind(command, Some(shortcut));
        self.settings.shortcuts = commands::stored_overrides();
        let keys = ctx.format_shortcut(&shortcut);
        let mut note = format!("{} is now {keys}.", command.label().trim_end_matches('…'));
        if !taken.is_empty() {
            let names: Vec<&str> = taken
                .iter()
                .map(|c| c.label().trim_end_matches('…'))
                .collect();
            note.push_str(&format!(" Removed it from {}.", names.join(", ")));
        }
        if bare && command.works_while_typing() {
            note.push_str(" Without Ctrl it does not work while typing in a text field.");
        }
        Some(note)
    }
}
