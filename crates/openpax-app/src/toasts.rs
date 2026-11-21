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
                    let offset = slide_offset(age, remaining);
                    let transform = egui::emath::TSTransform::from_translation(vec2(offset, 0.0));
                    ui.with_visual_transform(transform, |ui| {
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
        // Smooth frames while a toast is sliding, a slow tick otherwise.
        let moving = self.items.iter().any(|t| {
            let age = now - t.shown_at.unwrap_or(now);
            age < SLIDE_IN || t.level.lifetime() - age < SLIDE_OUT
        });
        if moving {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}

/// Seconds a toast takes to slide in.
const SLIDE_IN: f64 = 0.35;
/// Seconds a toast takes to drift out before it disappears.
const SLIDE_OUT: f64 = 0.5;
/// How far (in points) a toast travels while sliding.
const SLIDE_DISTANCE: f32 = 80.0;

/// Horizontal offset of a toast: it springs in from the right, overshooting a little,
/// and drifts back to the right while fading out.
fn slide_offset(age: f64, remaining: f64) -> f32 {
    let entering = (age / SLIDE_IN).clamp(0.0, 1.0) as f32;
    let leaving = 1.0 - (remaining / SLIDE_OUT).clamp(0.0, 1.0) as f32;
    (1.0 - ease_out_back(entering)) * SLIDE_DISTANCE + leaving * leaving * SLIDE_DISTANCE * 0.5
}

/// Eases from 0 to 1, going slightly past 1 before settling.
pub fn ease_out_back(t: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    let u = t - 1.0;
    1.0 + C3 * u * u * u + C1 * u * u
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

    #[test]
    fn toasts_slide_in_overshoot_and_settle() {
        assert!((slide_offset(0.0, 10.0) - SLIDE_DISTANCE).abs() < 1e-3);
        assert!(
            slide_offset(SLIDE_IN * 0.8, 10.0) < 0.0,
            "overshoots past its resting place"
        );
        assert!(slide_offset(SLIDE_IN, 10.0).abs() < 1e-3);
        assert!(slide_offset(5.0, 0.0) > 0.0, "drifts out at the end");
    }
}
