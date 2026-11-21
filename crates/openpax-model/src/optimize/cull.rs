//! Removing faces nobody can see, and tagging faces the game may hide.

use super::{EPS, Opacity, axis_of, is_valid_box, near, plane_axes};
use crate::{Direction, Element, Face};

/// Removes faces with no area, such as the edges of a flat plane. Returns how many.
pub(crate) fn remove_degenerate(elements: &mut [Element]) -> usize {
    let mut removed = 0;
    for element in elements {
        let before = element.faces.len();
        let (from, to) = (element.from, element.to);
        element.faces.retain(|&direction, _| {
            let (b, c) = plane_axes(axis_of(direction).0);
            (to[b] - from[b]).abs() > EPS && (to[c] - from[c]).abs() > EPS
        });
        removed += before - element.faces.len();
    }
    removed
}

/// Removes faces whose texture region is fully transparent. Returns how many.
pub(crate) fn remove_transparent(
    elements: &mut [Element],
    opacity: &dyn Fn(&Element, Direction, &Face) -> Opacity,
) -> usize {
    let mut removed = 0;
    for element in elements {
        let empty: Vec<Direction> = element
            .faces
            .iter()
            .filter(|(d, f)| opacity(element, **d, f) == Opacity::Empty)
            .map(|(d, _)| *d)
            .collect();
        for direction in &empty {
            element.faces.remove(direction);
        }
        removed += empty.len();
    }
    removed
}

/// Removes faces fully covered by solid boxes. Returns how many.
///
/// A box counts as solid when it is unrotated, has all six faces, and every face is opaque.
/// A face is hidden when the solids that reach past its plane, on the side it faces, cover it
/// completely. Decisions use the model as it was before this call, so two solids may hide each
/// other's touching faces: the outside of their union is never removed.
pub(crate) fn remove_hidden(
    elements: &mut [Element],
    opacity: &dyn Fn(&Element, Direction, &Face) -> Opacity,
) -> usize {
    let solid: Vec<bool> = elements
        .iter()
        .map(|e| {
            e.rotation.is_none()
                && is_valid_box(e)
                && e.faces.len() == 6
                && e.faces
                    .iter()
                    .all(|(d, f)| opacity(e, *d, f) == Opacity::Opaque)
        })
        .collect();

    let mut hidden: Vec<(usize, Direction)> = Vec::new();
    for (i, element) in elements.iter().enumerate() {
        if element.rotation.is_some() || !is_valid_box(element) {
            continue;
        }
        for &direction in element.faces.keys() {
            if is_covered(elements, &solid, i, direction) {
                hidden.push((i, direction));
            }
        }
    }
    for (i, direction) in &hidden {
        elements[*i].faces.remove(direction);
    }
    hidden.len()
}

fn is_covered(elements: &[Element], solid: &[bool], index: usize, direction: Direction) -> bool {
    let element = &elements[index];
    let (k, outward) = axis_of(direction);
    let plane = if outward {
        element.to[k]
    } else {
        element.from[k]
    };
    let (b, c) = plane_axes(k);
    let mut uncovered = vec![[
        element.from[b],
        element.to[b],
        element.from[c],
        element.to[c],
    ]];
    for (j, other) in elements.iter().enumerate() {
        if j == index || !solid[j] {
            continue;
        }
        let reaches_past = if outward {
            other.from[k] <= plane + EPS && other.to[k] > plane + EPS
        } else {
            other.to[k] >= plane - EPS && other.from[k] < plane - EPS
        };
        if reaches_past {
            subtract(
                &mut uncovered,
                [other.from[b], other.to[b], other.from[c], other.to[c]],
            );
            if uncovered.is_empty() {
                return true;
            }
        }
    }
    false
}

/// Removes `cut` from every rectangle `[x1, x2, y1, y2]` in `pieces`.
fn subtract(pieces: &mut Vec<[f32; 4]>, cut: [f32; 4]) {
    let mut out = Vec::with_capacity(pieces.len());
    for &[x1, x2, y1, y2] in pieces.iter() {
        let (ix1, ix2) = (x1.max(cut[0]), x2.min(cut[1]));
        let (iy1, iy2) = (y1.max(cut[2]), y2.min(cut[3]));
        if ix2 - ix1 <= EPS || iy2 - iy1 <= EPS {
            out.push([x1, x2, y1, y2]);
            continue;
        }
        if ix1 - x1 > EPS {
            out.push([x1, ix1, y1, y2]);
        }
        if x2 - ix2 > EPS {
            out.push([ix2, x2, y1, y2]);
        }
        if iy1 - y1 > EPS {
            out.push([ix1, ix2, y1, iy1]);
        }
        if y2 - iy2 > EPS {
            out.push([ix1, ix2, iy2, y2]);
        }
    }
    *pieces = out;
}

/// Gives faces on the outside of the block a `cullface`, as vanilla models do, so the game can
/// skip them next to full neighbours. Only faces lying entirely within the block's side are
/// tagged. Returns how many.
pub(crate) fn add_block_cullfaces(elements: &mut [Element]) -> usize {
    let mut added = 0;
    for element in elements {
        if element.rotation.is_some() || !is_valid_box(element) {
            continue;
        }
        let (from, to) = (element.from, element.to);
        for (&direction, face) in element.faces.iter_mut() {
            if face.cullface.is_some() {
                continue;
            }
            let (k, outward) = axis_of(direction);
            let on_side = if outward {
                near(to[k], 16.0)
            } else {
                near(from[k], 0.0)
            };
            let (b, c) = plane_axes(k);
            let inside = [b, c]
                .iter()
                .all(|&a| from[a] >= -EPS && to[a] <= 16.0 + EPS);
            if on_side && inside {
                face.cullface = Some(direction.name().to_owned());
                added += 1;
            }
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque(_: &Element, _: Direction, _: &Face) -> Opacity {
        Opacity::Opaque
    }

    fn cube(from: [f32; 3], to: [f32; 3]) -> Element {
        Element::cube(from, to, "#0")
    }

    #[test]
    fn touching_solids_hide_each_others_faces() {
        let mut elements = vec![
            cube([0.0; 3], [8.0, 8.0, 8.0]),
            cube([8.0, 0.0, 0.0], [16.0, 8.0, 8.0]),
        ];
        assert_eq!(remove_hidden(&mut elements, &opaque), 2);
        assert!(!elements[0].faces.contains_key(&Direction::East));
        assert!(!elements[1].faces.contains_key(&Direction::West));
        assert_eq!(elements[0].faces.len() + elements[1].faces.len(), 10);
    }

    #[test]
    fn partial_cover_is_kept_but_combined_cover_hides() {
        let mut elements = vec![
            cube([0.0; 3], [8.0, 8.0, 8.0]),
            cube([8.0, 0.0, 0.0], [16.0, 4.0, 8.0]),
        ];
        remove_hidden(&mut elements, &opaque);
        assert!(elements[0].faces.contains_key(&Direction::East));

        let mut elements = vec![
            cube([0.0; 3], [8.0, 8.0, 8.0]),
            cube([8.0, 0.0, 0.0], [16.0, 4.0, 8.0]),
            cube([8.0, 4.0, 0.0], [16.0, 8.0, 8.0]),
        ];
        remove_hidden(&mut elements, &opaque);
        assert!(!elements[0].faces.contains_key(&Direction::East));
    }

    #[test]
    fn open_or_see_through_boxes_hide_nothing() {
        let mut open = cube([8.0, 0.0, 0.0], [16.0, 8.0, 8.0]);
        open.faces.remove(&Direction::Up);
        let mut elements = vec![cube([0.0; 3], [8.0, 8.0, 8.0]), open];
        remove_hidden(&mut elements, &opaque);
        assert!(elements[0].faces.contains_key(&Direction::East));

        let mixed = |_: &Element, _: Direction, _: &Face| Opacity::Mixed;
        let mut elements = vec![
            cube([0.0; 3], [8.0, 8.0, 8.0]),
            cube([8.0, 0.0, 0.0], [16.0, 8.0, 8.0]),
        ];
        assert_eq!(remove_hidden(&mut elements, &mixed), 0);
    }

    #[test]
    fn a_box_inside_a_solid_loses_every_face() {
        let mut elements = vec![cube([0.0; 3], [16.0; 3]), cube([4.0; 3], [12.0; 3])];
        assert_eq!(remove_hidden(&mut elements, &opaque), 6);
        assert!(elements[1].faces.is_empty());
        assert_eq!(elements[0].faces.len(), 6);
    }

    #[test]
    fn degenerate_and_transparent_faces() {
        let mut elements = vec![cube([0.0, 2.0, 0.0], [16.0, 2.0, 16.0])];
        assert_eq!(remove_degenerate(&mut elements), 4);
        assert_eq!(elements[0].faces.len(), 2);

        let up_empty = |_: &Element, d: Direction, _: &Face| {
            if d == Direction::Up {
                Opacity::Empty
            } else {
                Opacity::Mixed
            }
        };
        assert_eq!(remove_transparent(&mut elements, &up_empty), 1);
        assert!(elements[0].faces.contains_key(&Direction::Down));
    }

    #[test]
    fn cullfaces_only_on_the_block_boundary() {
        let mut elements = vec![
            cube([0.0, 0.0, 6.0], [16.0, 4.0, 10.0]),
            cube([6.0, 0.0, 6.0], [10.0, 24.0, 10.0]),
        ];
        add_block_cullfaces(&mut elements);
        let tag = |e: usize, d: Direction| elements[e].faces[&d].cullface.clone();
        assert_eq!(tag(0, Direction::West).as_deref(), Some("west"));
        assert_eq!(tag(0, Direction::East).as_deref(), Some("east"));
        assert_eq!(tag(0, Direction::Down).as_deref(), Some("down"));
        assert_eq!(tag(0, Direction::Up), None);
        assert_eq!(tag(0, Direction::North), None);
        assert_eq!(tag(1, Direction::Down).as_deref(), Some("down"));
        assert_eq!(tag(1, Direction::Up), None, "sticks out of the block");
    }
}
