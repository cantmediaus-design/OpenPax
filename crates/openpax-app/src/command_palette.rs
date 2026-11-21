//! The command palette (Ctrl+Shift+P): type to find any command or asset, then press Enter.
//!
//! Commands come from the registry in [`crate::commands`], so everything in the menus is here,
//! including the commands without a shortcut, and only the ones that can run right now appear.

use eframe::egui::{self, Align, Align2, Key, RichText, vec2};

use crate::app::App;
use crate::commands::Command;
use crate::theme::{SPACE_M, SPACE_S};
use crate::{icons, widgets};

const WIDTH: f32 = 560.0;
const MAX_ROWS_HEIGHT: f32 = 380.0;
const MAX_ASSETS: usize = 50;

#[derive(Default)]
pub struct PaletteState {
    pub query: String,
    selected: usize,
    /// True until the palette has been drawn once (to focus the search field).
    fresh: bool,
}

impl PaletteState {
    pub fn new() -> Self {
        Self {
            fresh: true,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Command(Command),
    Asset(String),
}

/// Scores how well `query` matches `text`: `None` unless every query character appears in
/// order (ignoring case). Higher is better; consecutive runs and word starts score extra.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if query.is_empty() {
        return Some(0);
    }
    let chars: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let mut score = 0;
    let mut next = 0;
    let mut previous_match: Option<usize> = None;
    for (index, &c) in chars.iter().enumerate() {
        if next == query.len() {
            break;
        }
        if c != query[next] {
            continue;
        }
        score += 1;
        if previous_match == Some(index.wrapping_sub(1)) {
            score += 4;
        }
        let word_start =
            index == 0 || matches!(chars[index - 1], ' ' | '/' | '_' | '-' | '.' | ':');
        if word_start {
            score += 6;
        }
        previous_match = Some(index);
        next += 1;
    }
    (next == query.len()).then(|| score * 8 - chars.len() as i32)
}

impl App {
    fn palette_entries(&self, query: &str) -> Vec<Entry> {
        let mut commands: Vec<(i32, Command)> = Command::all()
            .filter(|c| *c != Command::OpenPalette && self.enabled(*c))
            .filter_map(|c| {
                let text = format!("{} {}", c.category(), c.label().trim_end_matches('…'));
                // Prefer matches on the label itself over matches that need the category.
                let score = fuzzy_score(query, c.label())
                    .map(|s| s + 4)
                    .or_else(|| fuzzy_score(query, &text))?;
                Some((score, c))
            })
            .collect();
        if !query.trim().is_empty() {
            commands.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        }
        let mut entries: Vec<Entry> = commands
            .into_iter()
            .map(|(_, c)| Entry::Command(c))
            .collect();

        // Assets only once something is typed, so the empty palette lists the commands.
        if let Some(pack) = &self.pack
            && !query.trim().is_empty()
        {
            let mut assets: Vec<(i32, &str)> = pack
                .index
                .entries
                .iter()
                .filter_map(|e| {
                    // A match on the file name beats one spread over the folders.
                    let name = e.rel_path.rsplit('/').next().unwrap_or_default();
                    let score = fuzzy_score(query, name)
                        .map(|s| s + 64)
                        .or_else(|| fuzzy_score(query, &e.rel_path))?;
                    Some((score, e.rel_path.as_str()))
                })
                .collect();
            assets.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
            entries.extend(
                assets
                    .into_iter()
                    .take(MAX_ASSETS)
                    .map(|(_, path)| Entry::Asset(path.to_owned())),
            );
        }
        entries
    }

    pub(crate) fn show_command_palette(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.palette.take() else {
            return;
        };
        let entries = self.palette_entries(&state.query);
        state.selected = state.selected.min(entries.len().saturating_sub(1));

        // Arrow keys move the highlight; read them before the text field sees them.
        let (down, up, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
            )
        });
        if !entries.is_empty() {
            if down {
                state.selected = (state.selected + 1) % entries.len();
            }
            if up {
                state.selected = (state.selected + entries.len() - 1) % entries.len();
            }
        }

        let mut chosen = enter
            .then(|| entries.get(state.selected).cloned())
            .flatten();
        let area = egui::Modal::default_area(egui::Id::unique("command_palette"))
            .anchor(Align2::CENTER_TOP, vec2(0.0, 72.0));
        let modal = egui::Modal::new(egui::Id::unique("command_palette"))
            .area(area)
            .show(ctx, |ui| {
                ui.set_width(WIDTH);
                let hint = widgets::icon_label(icons::SEARCH, "Type a command or a file name…");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut state.query)
                        .hint_text(hint)
                        .desired_width(f32::INFINITY)
                        .margin(vec2(SPACE_M, SPACE_M)),
                );
                if std::mem::take(&mut state.fresh) {
                    field.request_focus();
                }
                if field.changed() {
                    state.selected = 0;
                }
                ui.add_space(SPACE_S);

                if entries.is_empty() {
                    ui.add_space(SPACE_M);
                    ui.vertical_centered(|ui| ui.weak("Nothing matches"));
                    ui.add_space(SPACE_M);
                    return;
                }
                egui::ScrollArea::vertical()
                    .max_height(MAX_ROWS_HEIGHT)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let mut last_group = "";
                        for (index, entry) in entries.iter().enumerate() {
                            let group = match entry {
                                Entry::Command(_) => "COMMANDS",
                                Entry::Asset(_) => "FILES",
                            };
                            if group != last_group {
                                if !last_group.is_empty() {
                                    ui.add_space(SPACE_S);
                                }
                                widgets::caption(ui, group);
                                last_group = group;
                            }
                            let selected = index == state.selected;
                            let response = self.palette_row(ui, entry, selected);
                            if selected && (up || down) {
                                response.scroll_to_me(Some(Align::Center));
                            }
                            if response.clicked() {
                                chosen = Some(entry.clone());
                            }
                            if response.hovered()
                                && ui.input(|i| i.pointer.delta() != vec2(0.0, 0.0))
                            {
                                state.selected = index;
                            }
                        }
                    });
                ui.add_space(SPACE_S);
                ui.horizontal(|ui| {
                    ui.weak("↑↓ to move   Enter to run   Esc to close");
                });
            });

        if let Some(entry) = chosen {
            match entry {
                Entry::Command(command) => self.run(command, ctx),
                Entry::Asset(rel_path) => self.open_asset(&rel_path),
            }
        } else if !modal.should_close() {
            self.palette = Some(state);
        }
    }

    fn palette_row(&self, ui: &mut egui::Ui, entry: &Entry, selected: bool) -> egui::Response {
        let (glyph, label, detail, shortcut) = match entry {
            Entry::Command(command) => (
                command.icon(),
                command.label().to_owned(),
                command.category().to_owned(),
                command.shortcut().map(|s| ui.ctx().format_shortcut(&s)),
            ),
            Entry::Asset(rel_path) => {
                let kind = self
                    .pack
                    .as_ref()
                    .and_then(|p| p.index.entries.iter().find(|e| &e.rel_path == rel_path))
                    .map(|e| e.kind);
                let name = rel_path.rsplit('/').next().unwrap_or(rel_path).to_owned();
                (
                    kind.map_or(icons::FILE, icons::asset),
                    name,
                    widgets::shorten_path(rel_path, 48),
                    None,
                )
            }
        };
        let mut right = RichText::new(detail).weak();
        if let Some(shortcut) = shortcut {
            right = RichText::new(shortcut).monospace();
        }
        ui.add(
            egui::Button::new(widgets::icon_label(glyph, &label))
                .right_text(right)
                .selected(selected)
                .frame_when_inactive(selected)
                .min_size(vec2(ui.available_width(), 28.0)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matching_needs_every_character_in_order() {
        assert!(fuzzy_score("sv", "Save").is_some());
        assert!(fuzzy_score("SAVE", "save all").is_some());
        assert!(fuzzy_score("vs", "Save").is_none());
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert!(fuzzy_score("flip h", "Flip Horizontally").is_some());
    }

    #[test]
    fn word_starts_and_runs_rank_higher() {
        let exact = fuzzy_score("zoom", "Zoom In").unwrap();
        let scattered = fuzzy_score("zoom", "Resize Canvas of Our Map").unwrap_or(i32::MIN);
        assert!(exact > scattered);
        let initials = fuzzy_score("ld", "Load Default Pack").unwrap();
        let middle = fuzzy_score("ld", "Fold").unwrap();
        assert!(initials > middle);
        let file = fuzzy_score("stone", "assets/minecraft/textures/block/stone.png").unwrap();
        let other = fuzzy_score("stone", "assets/minecraft/textures/block/sandstone_top.png");
        assert!(other.is_none_or(|o| file > o));
    }
}
