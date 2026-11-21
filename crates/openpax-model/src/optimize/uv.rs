//! Face UVs as affine maps from model space to texture space.

use glam::{Vec2, Vec3};

use super::EPS;
use crate::geometry::{local_corners, texture_turns, vertex_uvs};
use crate::{Direction, Element, Face};

/// The affine map that places a face's texture on its plane: model units to 0..16 texture units.
#[derive(Clone, Copy, Debug)]
pub(crate) struct UvMap {
    origin: Vec3,
    /// Top-left to bottom-left edge, and the UV change along it.
    e1: Vec3,
    du: Vec2,
    /// Top-left to top-right edge, and the UV change along it.
    e2: Vec3,
    dv: Vec2,
    uv0: Vec2,
}

impl UvMap {
    /// `None` for faces without area, whose texture placement cannot be extended.
    pub(crate) fn new(element: &Element, direction: Direction, face: &Face) -> Option<Self> {
        let corners = local_corners(element, direction);
        let uvs = vertex_uvs(element, direction, face).map(Vec2::from);
        let e1 = corners[1] - corners[0];
        let e2 = corners[3] - corners[0];
        if e1.length_squared() < EPS * EPS || e2.length_squared() < EPS * EPS {
            return None;
        }
        Some(Self {
            origin: corners[0],
            e1,
            du: uvs[1] - uvs[0],
            e2,
            dv: uvs[3] - uvs[0],
            uv0: uvs[0],
        })
    }

    pub(crate) fn apply(&self, point: Vec3) -> Vec2 {
        let d = point - self.origin;
        self.uv0
            + self.du * (d.dot(self.e1) / self.e1.length_squared())
            + self.dv * (d.dot(self.e2) / self.e2.length_squared())
    }
}

/// Whether two maps place the texture identically on the given side of `element`.
pub(crate) fn maps_agree(a: &UvMap, b: &UvMap, element: &Element, direction: Direction) -> bool {
    local_corners(element, direction)
        .iter()
        .all(|&p| a.apply(p).abs_diff_eq(b.apply(p), EPS))
}

/// The `uv` value that reproduces `map` on the given side of `element` with `face`'s rotation.
pub(crate) fn uv_for(
    map: &UvMap,
    element: &Element,
    direction: Direction,
    face: &Face,
) -> [f32; 4] {
    let vertex = local_corners(element, direction).map(|p| map.apply(p));
    // `vertex_uvs` gives vertex i the corner (i + turns) % 4, so corner j sits at vertex j - turns.
    let turns = texture_turns(face);
    let corner = |j: usize| vertex[(j + 4 - turns) % 4];
    let (first, third) = (corner(0), corner(2));
    [first.x, first.y, third.x, third.y].map(clean)
}

/// Snaps float noise like `7.9999995` back to the value a person would have typed.
pub fn clean(value: f32) -> f32 {
    let rounded = (value * 1000.0).round() / 1000.0;
    if (rounded - value).abs() < EPS {
        rounded
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_round_trips_explicit_uvs_for_every_rotation() {
        let element = Element::cube([2.0, 3.0, 4.0], [10.0, 7.0, 12.0], "#0");
        for direction in Direction::ALL {
            for rotation in [0, 90, 180, 270] {
                let face = Face {
                    uv: Some([12.0, 1.0, 4.0, 5.0]),
                    rotation: Some(rotation),
                    texture: "#0".into(),
                    ..Default::default()
                };
                let map = UvMap::new(&element, direction, &face).unwrap();
                let uv = uv_for(&map, &element, direction, &face);
                assert_eq!(uv, face.uv.unwrap(), "{direction:?} {rotation}");
            }
        }
    }

    #[test]
    fn faces_without_area_have_no_map() {
        let element = Element::cube([0.0, 5.0, 0.0], [16.0, 5.0, 16.0], "#0");
        let face = &element.faces[&Direction::North];
        assert!(UvMap::new(&element, Direction::North, face).is_none());
        assert!(UvMap::new(&element, Direction::Up, face).is_some());
    }
}
