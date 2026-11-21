//! Making models cheaper to render without changing how they look.
//!
//! Every pass is conservative: when it cannot prove a change is invisible (rotated elements,
//! textures it cannot read, see-through pixels) it leaves the model alone. Texture pixels are
//! not available in this crate, so callers describe them through an opacity callback.

mod cull;
mod merge;
mod uv;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Direction, Element, Face, ModelFile};

pub use uv::clean as clean_uv;

/// Tolerance for comparing coordinates, in model units.
pub(crate) const EPS: f32 = 1e-4;

/// What the texels under a face's UV rectangle look like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opacity {
    /// Every texel is fully opaque, in every animation frame.
    Opaque,
    /// Every texel is fully transparent, in every animation frame.
    Empty,
    /// Some texels are see-through.
    Mixed,
    /// The texture could not be read.
    Unknown,
}

/// Which passes to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OptimizeOptions {
    /// Join touching elements whose faces continue seamlessly.
    pub merge_elements: bool,
    /// Remove faces with no area and faces buried inside solid elements.
    pub cull_hidden_faces: bool,
    /// Remove faces whose texture region is fully transparent.
    pub cull_transparent_faces: bool,
    /// Remove elements left without faces.
    pub remove_empty_elements: bool,
    /// Tag faces on the outside of a block model with `cullface`.
    pub add_block_cullfaces: bool,
    /// Remove texture variables no face uses.
    pub prune_texture_vars: bool,
}

impl Default for OptimizeOptions {
    fn default() -> Self {
        Self {
            merge_elements: true,
            cull_hidden_faces: true,
            cull_transparent_faces: true,
            remove_empty_elements: true,
            add_block_cullfaces: true,
            prune_texture_vars: true,
        }
    }
}

/// Facts about where the model sits in its pack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelContext {
    /// The file lives under `models/block/`, so it is placed in the world as a block.
    pub block_model: bool,
    /// Another model names this one as its `parent` and may rely on its texture variables.
    pub used_as_parent: bool,
}

impl ModelContext {
    /// Context for a pack-relative path such as `assets/ns/models/block/lamp.json`.
    pub fn for_path(rel_path: &str, used_as_parent: bool) -> Self {
        Self {
            block_model: rel_path.contains("/models/block/"),
            used_as_parent,
        }
    }
}

/// What [`optimize`] changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelReport {
    pub elements_before: usize,
    pub elements_after: usize,
    pub faces_before: usize,
    pub faces_after: usize,
    pub degenerate_faces: usize,
    pub transparent_faces: usize,
    pub hidden_faces: usize,
    pub merged_elements: usize,
    pub empty_elements: usize,
    pub cullfaces_added: usize,
    pub texture_vars_pruned: usize,
}

impl ModelReport {
    pub fn changed(&self) -> bool {
        self.degenerate_faces
            + self.transparent_faces
            + self.hidden_faces
            + self.merged_elements
            + self.empty_elements
            + self.cullfaces_added
            + self.texture_vars_pruned
            > 0
    }

    /// Adds another report's numbers to this one, for pack-wide totals.
    pub fn add(&mut self, other: &ModelReport) {
        self.elements_before += other.elements_before;
        self.elements_after += other.elements_after;
        self.faces_before += other.faces_before;
        self.faces_after += other.faces_after;
        self.degenerate_faces += other.degenerate_faces;
        self.transparent_faces += other.transparent_faces;
        self.hidden_faces += other.hidden_faces;
        self.merged_elements += other.merged_elements;
        self.empty_elements += other.empty_elements;
        self.cullfaces_added += other.cullfaces_added;
        self.texture_vars_pruned += other.texture_vars_pruned;
    }
}

/// Number of faces across all elements.
pub fn face_count(model: &ModelFile) -> usize {
    model.elements().iter().map(|e| e.faces.len()).sum()
}

/// Optimises `model` in place.
///
/// `opacity` is asked about a resolved texture location (`minecraft:block/stone`) and a UV
/// rectangle in 0..16 texture units (possibly mirrored). Faces whose texture variable this file
/// does not define are treated as [`Opacity::Unknown`] without asking.
pub fn optimize(
    model: &mut ModelFile,
    options: &OptimizeOptions,
    context: &ModelContext,
    opacity: &dyn Fn(&str, [f32; 4]) -> Opacity,
) -> ModelReport {
    let mut report = ModelReport {
        elements_before: model.elements().len(),
        faces_before: face_count(model),
        ..Default::default()
    };

    if let Some(mut elements) = model.elements.take() {
        let face_opacity = |element: &Element, direction: Direction, face: &Face| match model
            .resolve_texture(&face.texture)
        {
            Some(texture) => opacity(
                &texture,
                face.uv.unwrap_or_else(|| element.default_uv(direction)),
            ),
            None => Opacity::Unknown,
        };

        if options.cull_hidden_faces {
            report.degenerate_faces = cull::remove_degenerate(&mut elements);
        }
        if options.cull_transparent_faces {
            report.transparent_faces = cull::remove_transparent(&mut elements, &face_opacity);
        }
        // Merging can close boxes that then hide more faces, and culling can open the ends
        // that block merges, so alternate until neither finds anything.
        for _ in 0..8 {
            let mut changed = 0;
            if options.cull_hidden_faces {
                let hidden = cull::remove_hidden(&mut elements, &face_opacity);
                report.hidden_faces += hidden;
                changed += hidden;
            }
            if options.merge_elements {
                let merged = merge::merge_elements(&mut elements);
                report.merged_elements += merged;
                changed += merged;
            }
            if changed == 0 {
                break;
            }
        }
        if options.remove_empty_elements {
            let before = elements.len();
            elements.retain(|e| !e.faces.is_empty());
            report.empty_elements = before - elements.len();
        }
        if options.add_block_cullfaces && context.block_model {
            report.cullfaces_added = cull::add_block_cullfaces(&mut elements);
        }
        model.elements = Some(elements);
    }

    if options.prune_texture_vars && !context.used_as_parent && !model.elements().is_empty() {
        report.texture_vars_pruned = prune_texture_vars(model);
    }

    report.elements_after = model.elements().len();
    report.faces_after = face_count(model);
    report
}

/// Removes texture variables that no face uses, directly or through other variables.
/// `particle` is always kept. Returns how many were removed.
fn prune_texture_vars(model: &mut ModelFile) -> usize {
    let mut keep: BTreeSet<String> = BTreeSet::from(["particle".to_owned()]);
    for element in model.elements() {
        for face in element.faces.values() {
            if let Some(name) = face.texture.strip_prefix('#') {
                keep.insert(name.to_owned());
            }
        }
    }
    let mut pending: Vec<String> = keep.iter().cloned().collect();
    while let Some(name) = pending.pop() {
        if let Some(Value::String(value)) = model.textures.get(&name)
            && let Some(next) = value.strip_prefix('#')
            && keep.insert(next.to_owned())
        {
            pending.push(next.to_owned());
        }
    }
    let before = model.textures.len();
    model.textures.retain(|name, _| keep.contains(name));
    before - model.textures.len()
}

/// The axis a face's normal lies on, and whether it points towards positive coordinates.
pub(crate) fn axis_of(direction: Direction) -> (usize, bool) {
    match direction {
        Direction::West => (0, false),
        Direction::East => (0, true),
        Direction::Down => (1, false),
        Direction::Up => (1, true),
        Direction::North => (2, false),
        Direction::South => (2, true),
    }
}

/// The two axes spanning the plane perpendicular to axis `k`.
pub(crate) fn plane_axes(k: usize) -> (usize, usize) {
    match k {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

pub(crate) fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= EPS
}

/// `from` is not past `to` on any axis.
pub(crate) fn is_valid_box(element: &Element) -> bool {
    (0..3).all(|i| element.from[i] <= element.to[i] + EPS)
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::build_mesh;

    fn opaque(_: &str, _: [f32; 4]) -> Opacity {
        Opacity::Opaque
    }

    fn model(elements: Vec<Element>) -> ModelFile {
        let mut model = ModelFile {
            elements: Some(elements),
            ..Default::default()
        };
        model.textures.insert("0".into(), "test:block/a".into());
        model
    }

    #[test]
    fn a_sliced_block_becomes_one_cube() {
        let mut m = model(
            (0..4)
                .map(|i| {
                    let y = i as f32 * 4.0;
                    Element::cube([0.0, y, 0.0], [16.0, y + 4.0, 16.0], "#0")
                })
                .collect(),
        );
        let context = ModelContext::for_path("assets/test/models/block/a.json", false);
        let report = optimize(&mut m, &OptimizeOptions::default(), &context, &opaque);
        assert_eq!(m.elements().len(), 1);
        assert_eq!(
            (m.elements()[0].from, m.elements()[0].to),
            ([0.0; 3], [16.0; 3])
        );
        assert_eq!((report.faces_before, report.faces_after), (24, 6));
        assert_eq!(report.hidden_faces, 6);
        assert_eq!(report.merged_elements, 3);
        assert_eq!(report.cullfaces_added, 6);
        assert!(report.changed());
    }

    #[test]
    fn unknown_textures_block_culling_but_not_merging() {
        let mut m = model(vec![
            Element::cube([0.0; 3], [8.0, 16.0, 16.0], "#missing"),
            Element::cube([8.0, 0.0, 0.0], [16.0, 16.0, 16.0], "#missing"),
        ]);
        let report = optimize(
            &mut m,
            &OptimizeOptions::default(),
            &ModelContext::default(),
            &opaque,
        );
        assert_eq!(report.hidden_faces, 0);
        assert_eq!(m.elements().len(), 2, "faces between them stay");
    }

    #[test]
    fn rotated_elements_are_left_alone() {
        let mut inner = Element::cube([4.0; 3], [12.0; 3], "#0");
        inner.rotation = Some(crate::Rotation {
            angle: 45.0,
            ..Default::default()
        });
        let mut m = model(vec![Element::cube([0.0; 3], [16.0; 3], "#0"), inner]);
        let before = m.clone();
        optimize(
            &mut m,
            &OptimizeOptions {
                add_block_cullfaces: false,
                ..Default::default()
            },
            &ModelContext::default(),
            &opaque,
        );
        assert_eq!(m, before);
    }

    #[test]
    fn unused_texture_variables_are_pruned() {
        let mut m = model(vec![Element::cube([0.0; 3], [16.0; 3], "#side")]);
        for (k, v) in [
            ("side", "#base"),
            ("base", "test:block/b"),
            ("particle", "#p"),
            ("p", "test:block/c"),
            ("unused", "test:block/d"),
        ] {
            m.textures.insert(k.into(), v.into());
        }
        let mut parent = m.clone();
        optimize(
            &mut m,
            &OptimizeOptions::default(),
            &ModelContext::default(),
            &opaque,
        );
        let names: Vec<&str> = m.textures.keys().map(String::as_str).collect();
        assert_eq!(names, ["side", "base", "particle", "p"]);

        let context = ModelContext {
            used_as_parent: true,
            ..Default::default()
        };
        let report = optimize(&mut parent, &OptimizeOptions::default(), &context, &opaque);
        assert_eq!(report.texture_vars_pruned, 0);
    }

    #[test]
    fn options_switch_passes_off() {
        let mut m = model(vec![
            Element::cube([0.0; 3], [8.0, 16.0, 16.0], "#0"),
            Element::cube([8.0, 0.0, 0.0], [16.0, 16.0, 16.0], "#0"),
        ]);
        let none = OptimizeOptions {
            merge_elements: false,
            cull_hidden_faces: false,
            cull_transparent_faces: false,
            remove_empty_elements: false,
            add_block_cullfaces: false,
            prune_texture_vars: false,
        };
        let report = optimize(&mut m, &none, &ModelContext::default(), &opaque);
        assert!(!report.changed());
    }

    /// Nearest face hit by a ray: its texture and the UV under the hit point.
    fn trace(mesh: &[crate::FaceMesh], origin: Vec3, dir: Vec3) -> Option<(String, [f32; 2])> {
        let mut best: Option<(f32, String, [f32; 2])> = None;
        for face in mesh {
            let p = face.vertices.map(|v| Vec3::from(v.position));
            let uv = face.vertices.map(|v| glam::Vec2::from(v.uv));
            let normal = (p[1] - p[0]).cross(p[2] - p[0]);
            if normal.dot(dir) >= 0.0 {
                continue; // back faces are not drawn
            }
            for (a, b, c) in [(0, 1, 2), (0, 2, 3)] {
                let (ab, ac) = (p[b] - p[a], p[c] - p[a]);
                let q = dir.cross(ac);
                let det = ab.dot(q);
                if det.abs() < 1e-8 {
                    continue;
                }
                let t = origin - p[a];
                let s = t.dot(q) / det;
                let r = t.cross(ab);
                let w = dir.dot(r) / det;
                let distance = ac.dot(r) / det;
                if s < 0.0 || w < 0.0 || s + w > 1.0 || distance <= 0.0 {
                    continue;
                }
                if best.as_ref().is_none_or(|(d, ..)| distance < *d - 1e-4) {
                    let hit = uv[a] + (uv[b] - uv[a]) * s + (uv[c] - uv[a]) * w;
                    best = Some((
                        distance,
                        face.texture.clone().unwrap_or_default(),
                        hit.into(),
                    ));
                }
            }
        }
        best.map(|(_, t, uv)| (t, uv))
    }

    fn rays() -> Vec<(Vec3, Vec3)> {
        let mut out = Vec::new();
        let dirs = [
            Vec3::X,
            -Vec3::X,
            Vec3::Y,
            -Vec3::Y,
            Vec3::Z,
            -Vec3::Z,
            Vec3::new(0.3, -0.5, 0.8).normalize(),
            Vec3::new(-0.7, 0.2, -0.4).normalize(),
        ];
        for dir in dirs {
            for i in 0..24 {
                for j in 0..24 {
                    let (a, b) = (i as f32 * 0.79 - 1.13, j as f32 * 0.83 - 1.07);
                    // Start far outside the block, on the plane facing the ray.
                    let side = if dir.x.abs() > 0.9 {
                        Vec3::new(0.0, a, b)
                    } else if dir.y.abs() > 0.9 {
                        Vec3::new(a, 0.0, b)
                    } else {
                        Vec3::new(a, b, a * 0.5 + 3.1)
                    };
                    out.push((side - dir * 40.0 + dir.abs() * 8.0, dir));
                }
            }
        }
        out
    }

    /// Small deterministic generator, so failures reproduce.
    struct Lcg(u64);
    impl Lcg {
        fn below(&mut self, n: u32) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as u32) % n
        }
    }

    #[test]
    fn optimised_models_look_the_same() {
        let mut rng = Lcg(7);
        for round in 0..60 {
            let mut elements = Vec::new();
            for _ in 0..1 + rng.below(4) {
                // A box, sometimes sliced into pieces along one axis.
                let from = [0, 1, 2].map(|_| rng.below(10) as f32);
                let to = from.map(|f| f + 1.0 + rng.below(6) as f32);
                let k = rng.below(3) as usize;
                let slices = 1 + rng.below(3);
                let step = (to[k] - from[k]) / slices as f32;
                for s in 0..slices {
                    let (mut a, mut b) = (from, to);
                    a[k] = from[k] + step * s as f32;
                    b[k] = a[k] + step;
                    let mut e = Element::cube(a, b, "#0");
                    if rng.below(2) == 0 {
                        for (d, f) in e.faces.iter_mut() {
                            f.uv = Some(Element::cube(a, b, "").default_uv(*d));
                        }
                    }
                    if rng.below(5) == 0 {
                        let d = Direction::ALL[rng.below(6) as usize];
                        e.faces.remove(&d);
                    }
                    elements.push(e);
                }
            }
            let original = model(elements);
            let mut optimised = original.clone();
            let report = optimize(
                &mut optimised,
                &OptimizeOptions::default(),
                &ModelContext::for_path("assets/t/models/block/x.json", false),
                &opaque,
            );
            assert!(report.faces_after <= report.faces_before);
            let (before, after) = (build_mesh(&original), build_mesh(&optimised));
            for (origin, dir) in rays() {
                let (x, y) = (trace(&before, origin, dir), trace(&after, origin, dir));
                let same = match (&x, &y) {
                    (None, None) => true,
                    (Some((t1, a)), Some((t2, b))) => {
                        t1 == t2 && (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
                    }
                    _ => false,
                };
                assert!(
                    same,
                    "round {round}: ray {origin} {dir}: {x:?} vs {y:?}\n{}\n{}",
                    original.to_json(),
                    optimised.to_json()
                );
            }
        }
    }
}
