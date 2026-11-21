//! A burst of pixel confetti for moments worth celebrating (a new pack, a finished export).

use eframe::egui::{self, Color32, Id, LayerId, Order, Pos2, Shape, Stroke, Vec2, pos2, vec2};

/// Pieces fired from each of the two bottom corners.
const PIECES_PER_SIDE: usize = 60;
/// Seconds a burst lasts.
const LIFETIME: f32 = 2.6;
/// Seconds over which the pieces fade out at the end.
const FADE: f32 = 0.6;
/// Downward acceleration, in points per second squared.
const GRAVITY: f32 = 900.0;
/// Fraction of velocity kept after one second, so pieces float rather than fall like stones.
const DRAG_PER_SECOND: f32 = 0.35;

const COLORS: [Color32; 6] = [
    Color32::from_rgb(255, 140, 13),
    Color32::from_rgb(96, 190, 86),
    Color32::from_rgb(80, 140, 240),
    Color32::from_rgb(255, 210, 60),
    Color32::from_rgb(232, 86, 86),
    Color32::from_rgb(170, 110, 230),
];

#[derive(Clone, Copy, Debug)]
struct Piece {
    pos: Pos2,
    vel: Vec2,
    size: f32,
    angle: f32,
    spin: f32,
    /// Phase of the paper-flip wobble that squashes the square as it tumbles.
    flip: f32,
    flip_speed: f32,
    color: Color32,
}

#[derive(Default)]
pub struct Confetti {
    pieces: Vec<Piece>,
    /// `egui` time the current burst started.
    started: Option<f64>,
    /// Time of the previous frame, to step the simulation.
    last: f64,
    /// Set by [`Confetti::burst`]; the pieces are spawned on the next drawn frame,
    /// when the screen size is known.
    requested: bool,
    seed: u32,
}

impl Confetti {
    /// Fires a burst on the next frame.
    pub fn burst(&mut self) {
        self.requested = true;
    }

    pub fn is_active(&self) -> bool {
        self.requested || !self.pieces.is_empty()
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.is_active() {
            return;
        }
        let now = ctx.input(|i| i.time);
        let screen = ctx.content_rect();
        if self.requested {
            self.requested = false;
            self.seed = self.seed.wrapping_add(now.to_bits() as u32) | 1;
            self.spawn(screen);
            self.started = Some(now);
            self.last = now;
        }
        let dt = ((now - self.last) as f32).clamp(0.0, 1.0 / 20.0);
        self.last = now;
        let age = (now - self.started.unwrap_or(now)) as f32;
        if age > LIFETIME {
            self.pieces.clear();
            self.started = None;
            return;
        }
        self.step(dt);

        let fade = ((LIFETIME - age) / FADE).clamp(0.0, 1.0);
        let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("confetti")));
        for piece in &self.pieces {
            painter.add(piece_shape(piece, fade));
        }
        ctx.request_repaint();
    }

    fn spawn(&mut self, screen: egui::Rect) {
        self.pieces.clear();
        let mut rng = Rng(self.seed);
        let speed = screen.height().clamp(400.0, 1200.0);
        for side in [-1.0_f32, 1.0] {
            let origin = if side < 0.0 {
                screen.left_bottom()
            } else {
                screen.right_bottom()
            };
            for _ in 0..PIECES_PER_SIDE {
                // Aim up and in towards the middle of the window, fanned out a little.
                let angle = rng.range(55.0, 80.0).to_radians();
                let magnitude = speed * rng.range(0.9, 1.45);
                let vel = vec2(-side * angle.cos(), -angle.sin()) * magnitude;
                self.pieces.push(Piece {
                    pos: origin + vec2(-side * rng.range(0.0, 24.0), rng.range(0.0, 12.0)),
                    vel,
                    size: rng.range(5.0, 9.0).round(),
                    angle: rng.range(0.0, std::f32::consts::TAU),
                    spin: rng.range(-9.0, 9.0),
                    flip: rng.range(0.0, std::f32::consts::TAU),
                    flip_speed: rng.range(6.0, 14.0),
                    color: COLORS[rng.below(COLORS.len())],
                });
            }
        }
    }

    fn step(&mut self, dt: f32) {
        let drag = DRAG_PER_SECOND.powf(dt);
        for piece in &mut self.pieces {
            piece.vel.y += GRAVITY * dt;
            piece.vel *= drag;
            piece.pos += piece.vel * dt;
            piece.angle += piece.spin * dt;
            piece.flip += piece.flip_speed * dt;
        }
    }
}

/// A square, rotated and squashed along one axis so it looks like tumbling paper.
fn piece_shape(piece: &Piece, fade: f32) -> Shape {
    let half = piece.size / 2.0;
    let squash = piece.flip.cos().abs().max(0.15);
    let (sin, cos) = piece.angle.sin_cos();
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| {
        let local = vec2(x * half, y * half * squash);
        pos2(
            piece.pos.x + local.x * cos - local.y * sin,
            piece.pos.y + local.x * sin + local.y * cos,
        )
    });
    // The back of the paper is a little darker.
    let shade = if piece.flip.cos() < 0.0 { 0.75 } else { 1.0 };
    let color = piece.color.gamma_multiply(shade).gamma_multiply(fade);
    Shape::convex_polygon(corners.to_vec(), color, Stroke::NONE)
}

/// A tiny xorshift generator; confetti doesn't need anything better.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1u32 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_flies_up_then_falls_and_ends() {
        let mut confetti = Confetti::default();
        assert!(!confetti.is_active());
        confetti.burst();
        assert!(confetti.is_active());

        let screen = egui::Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0));
        confetti.seed = 12345;
        confetti.spawn(screen);
        assert_eq!(confetti.pieces.len(), PIECES_PER_SIDE * 2);
        assert!(
            confetti.pieces.iter().all(|p| p.vel.y < 0.0),
            "all fired upwards"
        );

        // Half a second in, they have risen into the window.
        for _ in 0..30 {
            confetti.step(1.0 / 60.0);
        }
        let mean_y: f32 =
            confetti.pieces.iter().map(|p| p.pos.y).sum::<f32>() / confetti.pieces.len() as f32;
        assert!(mean_y < screen.bottom() - 100.0);

        // A couple of seconds later, gravity has them on the way down.
        for _ in 0..120 {
            confetti.step(1.0 / 60.0);
        }
        assert!(confetti.pieces.iter().all(|p| p.vel.y > 0.0));
    }

    #[test]
    fn rng_stays_in_range() {
        let mut rng = Rng(7);
        for _ in 0..1000 {
            let v = rng.range(2.0, 3.0);
            assert!((2.0..3.0).contains(&v));
            assert!(rng.below(6) < 6);
        }
    }
}
