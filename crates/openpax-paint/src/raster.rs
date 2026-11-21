//! Rasterisation helpers shared by tools and selections.

/// Bresenham line from `a` to `b`, both ends included.
pub fn line_points(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let (mut x, mut y) = a;
    let dx = (b.0 - x).abs();
    let dy = -(b.1 - y).abs();
    let sx = if x < b.0 { 1 } else { -1 };
    let sy = if y < b.1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut out = Vec::with_capacity((dx - dy + 1) as usize);
    loop {
        out.push((x, y));
        if (x, y) == b {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    out
}

/// Removes the corner pixel of every L-shaped step, so freehand 1-pixel strokes never get
/// doubled-up corners (the "pixel-perfect" filter).
pub fn pixel_perfect(points: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = Vec::with_capacity(points.len());
    for &p in points {
        if out.last() == Some(&p) {
            continue;
        }
        out.push(p);
        let n = out.len();
        if n >= 3 {
            let (a, b, c) = (out[n - 3], out[n - 2], out[n - 1]);
            let ab = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
            let bc = ((c.0 - b.0).abs(), (c.1 - b.1).abs());
            let diagonal = (c.0 - a.0).abs() == 1 && (c.1 - a.1).abs() == 1;
            let orthogonal_steps = ab.0 + ab.1 == 1 && bc.0 + bc.1 == 1;
            if diagonal && orthogonal_steps {
                out.remove(n - 2);
            }
        }
    }
    out
}

/// Snaps the end of a line so its angle is a multiple of 15°... in pixel terms: horizontal,
/// vertical, or one of the clean pixel slopes (1:1, 2:1, 1:2).
pub fn constrain_line(start: (i32, i32), end: (i32, i32)) -> (i32, i32) {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let candidates: [(f64, f64); 8] = [
        (1.0, 0.0),
        (0.0, 1.0),
        (1.0, 1.0),
        (1.0, -1.0),
        (2.0, 1.0),
        (2.0, -1.0),
        (1.0, 2.0),
        (1.0, -2.0),
    ];
    let (fx, fy) = (f64::from(dx), f64::from(dy));
    let mut best = (0.0, (0, 0));
    for (cx, cy) in candidates {
        let len = (cx * cx + cy * cy).sqrt();
        let (ux, uy) = (cx / len, cy / len);
        let t = fx * ux + fy * uy;
        // Project onto the direction, then round to whole steps of the slope.
        let steps = (t / len).round();
        let p = ((cx * steps) as i32, (cy * steps) as i32);
        let score = t.abs();
        if score > best.0 {
            best = (score, p);
        }
    }
    (start.0 + best.1.0, start.1 + best.1.1)
}

/// Snaps a box drag to a square.
pub fn constrain_square(start: (i32, i32), end: (i32, i32)) -> (i32, i32) {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let side = dx.abs().max(dy.abs());
    let sx = if dx < 0 { -1 } else { 1 };
    let sy = if dy < 0 { -1 } else { 1 };
    (start.0 + side * sx, start.1 + side * sy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_perfect_removes_l_corners() {
        // A staircase drawn with orthogonal steps.
        let pts = [(0, 0), (1, 0), (1, 1), (2, 1), (2, 2)];
        assert_eq!(pixel_perfect(&pts), vec![(0, 0), (1, 1), (2, 2)]);
        // Straight lines are untouched.
        let straight = [(0, 0), (1, 0), (2, 0)];
        assert_eq!(pixel_perfect(&straight), straight.to_vec());
    }

    #[test]
    fn constrained_lines_snap() {
        assert_eq!(constrain_line((0, 0), (10, 1)), (10, 0));
        assert_eq!(constrain_line((0, 0), (10, 12)), (11, 11));
        assert_eq!(constrain_square((0, 0), (5, -2)), (5, -5));
    }
}
