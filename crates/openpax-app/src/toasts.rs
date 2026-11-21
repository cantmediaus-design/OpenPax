//! Non-blocking notifications stacked in the bottom-right corner.

use std::time::Duration;

use eframe::egui::{self, Align2, Color32, RichText, vec2};

use crate::theme::{self, SPACE_M, SPACE_S};
use crate::{icons, widgets};

const MAX_TOASTS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Warning,
    Error,
}

impl Level {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Info => icons::INFO,
            Self::Success => icons::SUCCESS,
            Self::Warning => icons::WARNING,
            Self::Error => icons::ERROR,
        }
    }

    pub fn color(self, ui: &egui::Ui) -> Color32 {
        let tokens = theme::of(ui);
        match self {
            Self::Info => tokens.info,
            Self::Success => tokens.success,
            Self::Warning => ui.visuals().warn_fg_color,
            Self::Error => ui.visuals().error_fg_color,
        }
    }

    /// Seconds a toast of this level stays up.
    fn lifetime(self) -> f64 {
        match self {
            Self::Info | Self::Success => 4.0,
            Self::Warning => 7.0,
            Self::Error => 10.0,
        }
    }
}

struct Toast {
    level: Level,
    text: String,
    /// `egui` time at which the toast was first drawn.
    shown_at: Option<f64>,
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, level: Level, text: impl Into<String>) {
        let text = text.into();
        // Repeating the same message refreshes it instead of stacking copies.
        self.items.retain(|t| t.text != text);
        self.items.push(Toast {
            level,
            text,
            shown_at: None,
        });
        if self.items.len() > MAX_TOASTS {
            self.items.remove(0);
        }
    }

    /// Draws the stack; `bottom_margin` keeps it clear of the status bar.
    pub fn show(&mut self, ctx: &egui::Context, bottom_margin: f32) {
        if self.items.is_empty() {
            return;
        }
        let now = ctx.input(|i| i.time);
        for toast in &mut self.items {
            toast.shown_at.get_or_insert(now);
        }
        self.items
            .retain(|t| now - t.shown_at.unwrap_or(now) < t.level.lifetime());

        let mut dismissed = None;
        egui::Area::new(egui::Id::unique("toasts"))
            .anchor(
                Align2::RIGHT_BOTTOM,
                vec2(-theme::SPACE_XL, -bottom_margin - SPACE_M),
            )
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = SPACE_M;
                for (index, toast) in self.items.iter().enumerate() {
                    let age = now - toast.shown_at.unwrap_or(now);
                    let remaining = toast.level.lifetime() - age;
                    // Fade in quickly, fade out over the last half second.
                    let opacity = (age / 0.15).min(remaining / 0.5).clamp(0.0, 1.0) as f32;
                    ui.scope(|ui| {
                        ui.set_opacity(opacity);
                        let color = toast.level.color(ui);
                        egui::Frame::popup(ui.style())
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.set_max_width(360.0);
                                ui.horizontal(|ui| {
                                    ui.label(widgets::icon(toast.level.icon()).color(color));
                                    ui.add(
                                        egui::Label::new(RichText::new(&toast.text))
                                            .wrap()
                                            .selectable(false),
                                    );
                                    ui.add_space(SPACE_S);
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(icons::CLOSE).size(12.0),
                                            )
                                            .frame_when_inactive(false),
                                        )
                                        .on_hover_text("Dismiss")
                                        .clicked()
                                    {
                                        dismissed = Some(index);
                                    }
                                });
                            });
                    });
                }
            });
        if let Some(index) = dismissed {
            self.items.remove(index);
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicates_are_merged_and_the_stack_is_bounded() {
        let mut toasts = Toasts::default();
        toasts.push(Level::Error, "a");
        toasts.push(Level::Error, "a");
        assert_eq!(toasts.items.len(), 1);
        for i in 0..10 {
            toasts.push(Level::Info, format!("message {i}"));
        }
        assert_eq!(toasts.items.len(), MAX_TOASTS);
        assert_eq!(toasts.items.last().unwrap().text, "message 9");
    }
}
