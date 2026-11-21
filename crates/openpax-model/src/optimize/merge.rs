//! Joining neighbouring boxes into one when nobody could tell the difference.

use serde_json::{Map, Value};

use super::uv::{UvMap, maps_agree, uv_for};
use super::{axis_of, is_valid_box, near, plane_axes};
use crate::{Direction, Element, Face};

/// Element keys that only modelling tools read; they don't stop two elements merging.
const EDITOR_ONLY_KEYS: [&str; 2] = ["color", "uuid"];

/// Merges touching elements and drops exact duplicates until nothing changes.
/// The merged element takes the place, name and editor data of the earlier one.
/// Returns how many elements were removed.
pub(crate) fn merge_elements(elements: &mut Vec<Element>) -> usize {
    let mut removed = 0;
    'search: loop {
        for i in 0..elements.len() {
            for j in i + 1..elements.len() {
                let merged = if same_ignoring_editor_data(&elements[i], &elements[j]) {
                    Some(elements[i].clone())
                } else {
                    try_merge(&elements[i], &elements[j])
                };
                if let Some(merged) = merged {
                    elements[i] = merged;
                    elements.remove(j);
                    removed += 1;
                    continue 'search;
                }
            }
        }
        return removed;
    }
}

fn same_ignoring_editor_data(a: &Element, b: &Element) -> bool {
    a.from == b.from
        && a.to == b.to
        && a.rotation == b.rotation
        && a.faces == b.faces
        && same_extra(&a.extra, &b.extra)
}

fn same_extra(a: &Map<String, Value>, b: &Map<String, Value>) -> bool {
    let relevant = |m: &Map<String, Value>| -> Vec<(String, Value)> {
        let mut v: Vec<_> = m
            .iter()
            .filter(|(k, _)| !EDITOR_ONLY_KEYS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        v.sort_by(|x, y| x.0.cmp(&y.0));
        v
    };
    relevant(a) == relevant(b)
}

/// `a` and `b` joined into one box, if they touch along one axis with identical cross
/// sections, the touching ends have no faces, and every side face continues seamlessly.
pub(crate) fn try_merge(a: &Element, b: &Element) -> Option<Element> {
    if a.rotation != b.rotation
        || !same_extra(&a.extra, &b.extra)
        || !is_valid_box(a)
        || !is_valid_box(b)
    {
        return None;
    }
    for k in 0..3 {
        let (p, q) = plane_axes(k);
        let same_section = [p, q]
            .iter()
            .all(|&o| near(a.from[o], b.from[o]) && near(a.to[o], b.to[o]));
        if !same_section {
            continue;
        }
        let (lo, hi) = if near(a.to[k], b.from[k]) {
            (a, b)
        } else if near(b.to[k], a.from[k]) {
            (b, a)
        } else {
            continue;
        };
        if let Some(merged) = merge_along(a, lo, hi, k) {
            return Some(merged);
        }
    }
    None
}

fn merge_along(base: &Element, lo: &Element, hi: &Element, k: usize) -> Option<Element> {
    let [negative, positive] = ends(k);
    if lo.faces.contains_key(&positive) || hi.faces.contains_key(&negative) {
        return None;
    }
    let mut merged = base.clone();
    merged.from[k] = lo.from[k];
    merged.to[k] = hi.to[k];
    merged.faces.clear();
    if let Some(face) = lo.faces.get(&negative) {
        merged.faces.insert(negative, face.clone());
    }
    if let Some(face) = hi.faces.get(&positive) {
        merged.faces.insert(positive, face.clone());
    }
    for direction in Direction::ALL {
        if axis_of(direction).0 == k {
            continue;
        }
        match (lo.faces.get(&direction), hi.faces.get(&direction)) {
            (None, None) => {}
            (Some(f), Some(g)) => {
                let face = merge_face(lo, f, hi, g, &merged, direction)?;
                merged.faces.insert(direction, face);
            }
            _ => return None,
        }
    }
    Some(merged)
}

/// The face covering both `f` (on `lo`) and `g` (on `hi`), if their textures line up.
fn merge_face(
    lo: &Element,
    f: &Face,
    hi: &Element,
    g: &Face,
    merged: &Element,
    direction: Direction,
) -> Option<Face> {
    let same_settings = f.texture == g.texture
        && f.cullface == g.cullface
        && f.rotation.unwrap_or(0).rem_euclid(360) == g.rotation.unwrap_or(0).rem_euclid(360)
        && f.tintindex == g.tintindex
        && f.extra == g.extra;
    if !same_settings {
        return None;
    }
    let map_f = UvMap::new(lo, direction, f)?;
    let map_g = UvMap::new(hi, direction, g)?;
    if !maps_agree(&map_f, &map_g, merged, direction) {
        return None;
    }
    let mut face = f.clone();
    if f.uv.is_some() || g.uv.is_some() {
        face.uv = Some(uv_for(&map_f, merged, direction, f));
    }
    Some(face)
}

/// The faces at the low and high end of axis `k`.
fn ends(k: usize) -> [Direction; 2] {
    match k {
        0 => [Direction::West, Direction::East],
        1 => [Direction::Down, Direction::Up],
        _ => [Direction::North, Direction::South],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rotation;

    /// A box without the faces at the ends of axis `k`, so neighbours along it can merge.
    fn segment(from: [f32; 3], to: [f32; 3], open: &[Direction]) -> Element {
        let mut e = Element::cube(from, to, "#0");
        for d in open {
            e.faces.remove(d);
        }
        e
    }

    #[test]
    fn auto_uv_segments_merge_and_keep_auto_uv() {
        let a = segment([0.0; 3], [4.0, 4.0, 4.0], &[Direction::East]);
        let b = segment([4.0, 0.0, 0.0], [10.0, 4.0, 4.0], &[Direction::West]);
        let merged = try_merge(&a, &b).unwrap();
        assert_eq!((merged.from, merged.to), ([0.0; 3], [10.0, 4.0, 4.0]));
        assert_eq!(merged.faces.len(), 6);
        assert!(merged.faces.values().all(|f| f.uv.is_none()));
        assert!(try_merge(&b, &a).is_some(), "order does not matter");
    }

    #[test]
    fn explicit_uvs_merge_when_continuous() {
        let mut a = segment([0.0; 3], [4.0, 4.0, 4.0], &[Direction::East]);
        let mut b = segment([4.0, 0.0, 0.0], [8.0, 4.0, 4.0], &[Direction::West]);
        // Mirrored and rotated, continuing across the seam on the south face.
        for (e, uv) in [
            (&mut a, [8.0, 0.0, 4.0, 4.0]),
            (&mut b, [4.0, 0.0, 0.0, 4.0]),
        ] {
            let face = e.faces.get_mut(&Direction::South).unwrap();
            face.uv = Some(uv);
        }
        let merged = try_merge(&a, &b).unwrap();
        assert_eq!(
            merged.faces[&Direction::South].uv,
            Some([8.0, 0.0, 0.0, 4.0])
        );

        b.faces.get_mut(&Direction::South).unwrap().uv = Some([4.0, 0.0, 0.0, 8.0]);
        assert!(try_merge(&a, &b).is_none(), "different texel scale");
    }

    #[test]
    fn rotated_face_textures_merge() {
        let mut a = segment([0.0; 3], [4.0, 4.0, 4.0], &[Direction::Up]);
        let mut b = segment([0.0, 4.0, 0.0], [4.0, 8.0, 4.0], &[Direction::Down]);
        for e in [&mut a, &mut b] {
            let uv = e.default_uv(Direction::North);
            let face = e.faces.get_mut(&Direction::North).unwrap();
            face.uv = Some(uv);
            face.rotation = Some(90);
        }
        // With a quarter turn the texture's u axis runs along the model's y axis, so the
        // two default rectangles are no longer continuous.
        assert!(try_merge(&a, &b).is_none());

        b.faces.get_mut(&Direction::North).unwrap().uv = Some([8.0, 12.0, 12.0, 16.0]);
        a.faces.get_mut(&Direction::North).unwrap().uv = Some([12.0, 12.0, 16.0, 16.0]);
        let merged = try_merge(&a, &b).unwrap();
        assert_eq!(
            merged.faces[&Direction::North].uv,
            Some([8.0, 12.0, 16.0, 16.0])
        );
    }

    #[test]
    fn blockers() {
        let a = Element::cube([0.0; 3], [4.0; 3], "#0");
        let b = Element::cube([4.0, 0.0, 0.0], [8.0, 4.0, 4.0], "#0");
        assert!(try_merge(&a, &b).is_none(), "faces between them");

        let a = segment([0.0; 3], [4.0; 3], &[Direction::East]);
        let mut b = segment([4.0, 0.0, 0.0], [8.0, 4.0, 4.0], &[Direction::West]);
        b.faces.get_mut(&Direction::Up).unwrap().texture = "#1".into();
        assert!(try_merge(&a, &b).is_none(), "different texture");

        let b = segment([4.0, 0.0, 0.0], [8.0, 5.0, 4.0], &[Direction::West]);
        assert!(try_merge(&a, &b).is_none(), "different cross section");

        let mut b = segment([4.0, 0.0, 0.0], [8.0, 4.0, 4.0], &[Direction::West]);
        b.rotation = Some(Rotation {
            angle: 22.5,
            ..Default::default()
        });
        assert!(try_merge(&a, &b).is_none(), "different rotation");
    }

    #[test]
    fn merging_runs_to_a_fixed_point_and_removes_duplicates() {
        let ends = [Direction::East, Direction::West];
        let mut elements = vec![
            segment([0.0; 3], [2.0, 4.0, 4.0], &[Direction::East]),
            segment([2.0, 0.0, 0.0], [4.0, 4.0, 4.0], &ends),
            segment([4.0, 0.0, 0.0], [6.0, 4.0, 4.0], &ends),
            segment([6.0, 0.0, 0.0], [8.0, 4.0, 4.0], &[Direction::West]),
            Element::cube([10.0; 3], [12.0; 3], "#0"),
            Element::cube([10.0; 3], [12.0; 3], "#0"),
        ];
        elements[0].name = Some("first".into());
        assert_eq!(merge_elements(&mut elements), 4);
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0].name.as_deref(), Some("first"));
        assert_eq!(
            (elements[0].from, elements[0].to),
            ([0.0; 3], [8.0, 4.0, 4.0])
        );
    }

    #[test]
    fn editor_colours_do_not_block_merging() {
        let mut a = segment([0.0; 3], [4.0; 3], &[Direction::East]);
        let mut b = segment([4.0, 0.0, 0.0], [8.0, 4.0, 4.0], &[Direction::West]);
        a.extra.insert("color".into(), 1.into());
        b.extra.insert("color".into(), 5.into());
        assert!(try_merge(&a, &b).is_some());
        b.extra.insert("shade".into(), false.into());
        assert!(try_merge(&a, &b).is_none());
    }
}
