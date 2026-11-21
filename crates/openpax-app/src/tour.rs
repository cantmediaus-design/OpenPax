//! A short guided tour the first time a pack is open: it outlines the explorer, the editor
//! area and the inspector in turn, with a card explaining each. Help → Show the Tour replays it.

use eframe::egui::{self, Align2, Rect, RichText, Stroke, StrokeKind, pos2};

use crate::app::App;
use crate::commands::Command;
use crate::theme::{self, SPACE_M, SPACE_S};
use crate::{icons, widgets};

const CARD_WIDTH: f32 = 300.0;

/// Screen areas the tour points at, recorded while the panels are laid out each frame.
#[derive(Clone, Copy, Default)]
pub struct Layout {
    pub explorer: Option<Rect>,
    pub central: Option<Rect>,
    pub inspector: Option<Rect>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Explorer,
    Central,
    Inspector,
    Screen,
}

struct Step {
    target: Target,
    title: &'static str,
    text: &'static str,
}

const STEPS: [Step; 4] = [
    Step {
        target: Target::Explorer,
        title: "Your pack's files",
        text: "Everything in the pack, by namespace and type. Click a file to open it; \
               right-click for rename, duplicate and delete. The filter and grid buttons \
               above help in big packs.",
    },
    Step {
        target: Target::Central,
        title: "Editors",
        text: "Textures open in the pixel editor, models in the 3D editor and everything \
               else as text. Each file gets a tab; drag tabs to split the view.",
    },
    Step {
        target: Target::Inspector,
        title: "The inspector",
        text: "Colours, palette and layers for textures, element properties for models, \
               and the pack's own settings and icon at the bottom.",
    },
    Step {
        target: Target::Screen,
        title: "Find anything",
        text: "The Command Palette runs any command or opens any file by name. Help → \
               Keyboard Shortcuts lists (and changes) every key.",
    },
];

impl App {
    pub(crate) fn start_tour(&mut self) {
        self.tour = Some(0);
    }

    /// Draws the current tour step over everything else.
    pub(crate) fn show_tour(&mut self, ctx: &egui::Context) {
        let Some(index) = self.tour else { return };
        if self.pack.is_none() || !self.dialogs.is_empty() {
            return; // wait until the pack is open and dialogs are dealt with
        }
        let Some(step) = STEPS.get(index) else {
            self.end_tour();
            return;
        };
        let screen = ctx.content_rect();
        let target = match step.target {
            Target::Explorer => self.layout.explorer,
            Target::Central => self.layout.central,
            Target::Inspector => self.layout.inspector,
            Target::Screen => None,
        };

        // Dim everything, then outline the part being explained.
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("tour_backdrop"),
        ));
        painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(90));
        let accent = theme::tokens(ctx.theme() == egui::Theme::Dark).accent;
        if let Some(rect) = target {
            painter.rect_stroke(
                rect.shrink(2.0),
                theme::RADIUS_L,
                Stroke::new(2.5, accent),
                StrokeKind::Inside,
            );
        }

        // Put the card beside the outlined area, or in the middle of the window.
        let (anchor, pivot) = match (step.target, target) {
            (Target::Explorer, Some(rect)) => (
                pos2(rect.right() + SPACE_M, rect.top() + 48.0),
                Align2::LEFT_TOP,
            ),
            (Target::Inspector, Some(rect)) => (
                pos2(rect.left() - SPACE_M, rect.top() + 48.0),
                Align2::RIGHT_TOP,
            ),
            (_, Some(rect)) => (rect.center(), Align2::CENTER_CENTER),
            (_, None) => (screen.center(), Align2::CENTER_CENTER),
        };
        let palette_keys = Command::OpenPalette
            .shortcut()
            .map(|s| ctx.format_shortcut(&s));

        let mut next = false;
        let mut skip = false;
        egui::Area::new(egui::Id::unique("tour_card"))
            .order(egui::Order::Tooltip)
            .fixed_pos(anchor)
            .pivot(pivot)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(SPACE_M + SPACE_S)
                    .show(ui, |ui| {
                        ui.set_width(CARD_WIDTH);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icons::LIGHTBULB).size(18.0).color(accent));
                            ui.label(RichText::new(step.title).strong().size(16.0));
                        });
                        ui.add_space(SPACE_S);
                        ui.label(step.text);
                        if step.target == Target::Screen
                            && let Some(keys) = &palette_keys
                        {
                            ui.add_space(SPACE_S);
                            ui.label(RichText::new(format!("Command Palette: {keys}")).monospace());
                        }
                        ui.add_space(SPACE_M);
                        egui::Sides::new().show(
                            ui,
                            |ui| {
                                ui.weak(format!("{} of {}", index + 1, STEPS.len()));
                            },
                            |ui| {
                                let last = index + 1 == STEPS.len();
                                let label = if last { "Done" } else { "Next" };
                                next = widgets::primary_button(ui, true, label).clicked();
                                if !last {
                                    skip = widgets::secondary_button(ui, "Skip Tour").clicked();
                                }
                            },
                        );
                    });
            });

        let (enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if skip || escape {
            self.end_tour();
        } else if next || enter {
            self.tour = Some(index + 1);
            if index + 1 >= STEPS.len() {
                self.end_tour();
            }
        }
    }

    fn end_tour(&mut self) {
        self.tour = None;
        self.settings.tour_done = true;
    }
}
