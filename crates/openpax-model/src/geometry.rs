use glam::{Mat4, Quat, Vec3};

use crate::{Axis, Direction, Element, Face, ModelFile};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    /// Position in model units (a full block spans 0..16).
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Texture coordinates in 0..1.
    pub uv: [f32; 2],
}

/// One textured quad of an element.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceMesh {
    pub element: usize,
    pub direction: Direction,
    /// The face's texture resolved to a resource location, if this file defines it.
    pub texture: Option<String>,
    /// Top-left, bottom-left, bottom-right, top-right as seen from outside (counter-clockwise).
    pub vertices: [Vertex; 4],
}

/// Result of a successful [`pick`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub element: usize,
    pub direction: Direction,
    /// Distance along the ray, in units of the ray direction's length.
    pub distance: f32,
}

impl Element {
    /// The transform applied by the element's `rotation`, in model units.
    pub fn transform(&self) -> Mat4 {
        let Some(rotation) = &self.rotation else {
            return Mat4::IDENTITY;
        };
        let axis = match rotation.axis {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
        };
        let angle = rotation.angle.to_radians();
        // `rescale` stretches the faces across the whole block after rotating.
        let scale = if rotation.rescale == Some(true) && angle.cos().abs() > 1e-4 {
            let stretch = 1.0 / angle.cos().abs();
            Vec3::splat(stretch) - axis * (stretch - 1.0)
        } else {
            Vec3::ONE
        };
        let origin = Vec3::from(rotation.origin);
        Mat4::from_translation(origin)
            * Mat4::from_quat(Quat::from_axis_angle(axis, angle))
            * Mat4::from_scale(scale)
            * Mat4::from_translation(-origin)
    }
}

/// The untransformed corners of a face: top-left, bottom-left, bottom-right, top-right.
pub(crate) fn local_corners(element: &Element, direction: Direction) -> [Vec3; 4] {
    let [x1, y1, z1] = element.from;
    let [x2, y2, z2] = element.to;
    let v = Vec3::new;
    match direction {
        Direction::North => [v(x2, y2, z1), v(x2, y1, z1), v(x1, y1, z1), v(x1, y2, z1)],
        Direction::South => [v(x1, y2, z2), v(x1, y1, z2), v(x2, y1, z2), v(x2, y2, z2)],
        Direction::West => [v(x1, y2, z1), v(x1, y1, z1), v(x1, y1, z2), v(x1, y2, z2)],
        Direction::East => [v(x2, y2, z2), v(x2, y1, z2), v(x2, y1, z1), v(x2, y2, z1)],
        Direction::Up => [v(x1, y2, z1), v(x1, y2, z2), v(x2, y2, z2), v(x2, y2, z1)],
        Direction::Down => [v(x1, y1, z2), v(x1, y1, z1), v(x2, y1, z1), v(x2, y1, z2)],
    }
}

/// The corners of one side of an element after its rotation, whether or not the face exists.
pub fn element_corners(element: &Element, direction: Direction) -> [[f32; 3]; 4] {
    let transform = element.transform();
    local_corners(element, direction).map(|p| transform.transform_point3(p).to_array())
}

/// Quarter turns of a face's texture rotation (0..=3).
pub(crate) fn texture_turns(face: &Face) -> usize {
    (face.rotation.unwrap_or(0).rem_euclid(360) / 90) as usize
}

/// The UV (in 0..16 texture units) each corner of a face receives, in [`local_corners`] order.
pub(crate) fn vertex_uvs(element: &Element, direction: Direction, face: &Face) -> [[f32; 2]; 4] {
    let [u1, v1, u2, v2] = face.uv.unwrap_or_else(|| element.default_uv(direction));
    let uvs = [[u1, v1], [u1, v2], [u2, v2], [u2, v1]];
    // Rotating the texture clockwise shifts which UV corner each vertex receives.
    let turns = texture_turns(face);
    std::array::from_fn(|i| uvs[(i + turns) % 4])
}

/// Builds the quads for every face the model defines.
pub fn build_mesh(model: &ModelFile) -> Vec<FaceMesh> {
    let mut out = Vec::new();
    for (index, element) in model.elements().iter().enumerate() {
        let transform = element.transform();
        for (&direction, face) in &element.faces {
            let uvs = vertex_uvs(element, direction, face);
            let normal = transform
                .transform_vector3(Vec3::from(direction.normal()))
                .normalize_or_zero();
            let corners = local_corners(element, direction);
            let vertices = std::array::from_fn(|i| {
                let [u, v] = uvs[i];
                Vertex {
                    position: transform.transform_point3(corners[i]).to_array(),
                    normal: normal.to_array(),
                    uv: [u / 16.0, v / 16.0],
                }
            });
            out.push(FaceMesh {
                element: index,
                direction,
                texture: model.resolve_texture(&face.texture),
                vertices,
            });
        }
    }
    out
}

/// Möller–Trumbore ray/triangle intersection; returns the distance along the ray.
fn ray_triangle(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (ab, ac) = (b - a, c - a);
    let p = dir.cross(ac);
    let det = ab.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv = 1.0 / det;
    let t = origin - a;
    let u = t.dot(p) * inv;
    let q = t.cross(ab);
    let v = dir.dot(q) * inv;
    let distance = ac.dot(q) * inv;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && distance > 1e-5).then_some(distance)
}

/// Finds the nearest element side hit by a ray. All six sides of every element count,
/// so elements stay selectable even where they have no face.
pub fn pick(model: &ModelFile, origin: [f32; 3], direction: [f32; 3]) -> Option<Hit> {
    let (origin, dir) = (Vec3::from(origin), Vec3::from(direction));
    let mut best: Option<Hit> = None;
    for (index, element) in model.elements().iter().enumerate() {
        for side in Direction::ALL {
            let [a, b, c, d] = element_corners(element, side).map(Vec3::from);
            let hit = ray_triangle(origin, dir, a, b, c).or(ray_triangle(origin, dir, a, c, d));
            if let Some(distance) = hit
                && best.is_none_or(|h| distance < h.distance)
            {
                best = Some(Hit {
                    element: index,
                    direction: side,
                    distance,
                });
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rotation;

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    fn model(elements: Vec<Element>) -> ModelFile {
        let mut model = ModelFile {
            elements: Some(elements),
            ..Default::default()
        };
        model
            .textures
            .insert("0".into(), "minecraft:block/stone".into());
        model
    }

    #[test]
    fn faces_wind_counter_clockwise_and_face_outwards() {
        let mesh = build_mesh(&model(vec![Element::cube(
            [2.0, 0.0, 4.0],
            [10.0, 6.0, 16.0],
            "#0",
        )]));
        assert_eq!(mesh.len(), 6);
        for face in &mesh {
            let [a, b, c, _] = face.vertices.map(|v| Vec3::from(v.position));
            let winding_normal = (b - a).cross(c - a).normalize();
            let expected = Vec3::from(face.direction.normal());
            assert!(winding_normal.dot(expected) > 0.999, "{:?}", face.direction);
            assert_eq!(face.vertices[0].normal, face.direction.normal());
            assert_eq!(face.texture.as_deref(), Some("minecraft:block/stone"));
        }
    }

    #[test]
    fn uv_corners_and_texture_rotation() {
        let mut element = Element::cube([0.0; 3], [16.0; 3], "#0");
        element.faces.insert(
            Direction::North,
            Face {
                uv: Some([0.0, 0.0, 8.0, 16.0]),
                texture: "#0".into(),
                ..Default::default()
            },
        );
        let north = |m: &ModelFile| {
            build_mesh(m)
                .into_iter()
                .find(|f| f.direction == Direction::North)
                .unwrap()
        };
        let mut m = model(vec![element]);
        let uvs = north(&m).vertices.map(|v| v.uv);
        assert_eq!(uvs, [[0.0, 0.0], [0.0, 1.0], [0.5, 1.0], [0.5, 0.0]]);
        // Top-left of the north face is the +x, +y corner when looking from the north.
        assert_eq!(north(&m).vertices[0].position, [16.0, 16.0, 0.0]);

        m.elements.as_mut().unwrap()[0]
            .faces
            .get_mut(&Direction::North)
            .unwrap()
            .rotation = Some(90);
        let rotated = north(&m).vertices.map(|v| v.uv);
        assert_eq!(rotated, [[0.0, 1.0], [0.5, 1.0], [0.5, 0.0], [0.0, 0.0]]);
    }

    #[test]
    fn missing_uv_uses_defaults_and_unknown_texture_is_none() {
        let mut element = Element::cube([0.0, 0.0, 0.0], [8.0, 4.0, 16.0], "#nope");
        element.faces.retain(|d, _| *d == Direction::Up);
        let mesh = build_mesh(&model(vec![element]));
        assert_eq!(mesh.len(), 1, "only defined faces are meshed");
        assert_eq!(mesh[0].texture, None);
        assert_eq!(mesh[0].vertices[2].uv, [0.5, 1.0]);
    }

    #[test]
    fn rotation_about_y_axis() {
        let mut element = Element::cube([8.0, 0.0, 8.0], [16.0, 1.0, 9.0], "#0");
        element.rotation = Some(Rotation {
            angle: 90.0,
            axis: Axis::Y,
            ..Default::default()
        });
        // +x from the pivot rotates to -z for a right-handed turn about +y.
        let p = element
            .transform()
            .transform_point3(Vec3::new(16.0, 0.0, 8.0));
        assert!(near(p.to_array(), [8.0, 0.0, 0.0]), "{p:?}");
    }

    #[test]
    fn rescale_stretches_perpendicular_axes() {
        let mut element = Element::cube([0.0; 3], [16.0; 3], "#0");
        element.rotation = Some(Rotation {
            angle: 45.0,
            axis: Axis::Y,
            rescale: Some(true),
            ..Default::default()
        });
        let p = element
            .transform()
            .transform_point3(Vec3::new(16.0, 3.0, 8.0));
        // 8 units from the pivot, stretched by sqrt(2), then turned 45 degrees.
        assert!(near(p.to_array(), [16.0, 3.0, 0.0]), "{p:?}");
    }

    #[test]
    fn pick_returns_nearest_side() {
        let m = model(vec![
            Element::cube([0.0, 0.0, 0.0], [4.0, 4.0, 4.0], "#0"),
            Element::cube([0.0, 0.0, 8.0], [4.0, 4.0, 12.0], "#0"),
        ]);
        let hit = pick(&m, [2.0, 2.0, 30.0], [0.0, 0.0, -1.0]).unwrap();
        assert_eq!((hit.element, hit.direction), (1, Direction::South));
        assert!((hit.distance - 18.0).abs() < 1e-4);

        let hit = pick(&m, [2.0, 20.0, 2.0], [0.0, -1.0, 0.0]).unwrap();
        assert_eq!((hit.element, hit.direction), (0, Direction::Up));
        assert_eq!(pick(&m, [10.0, 2.0, 30.0], [0.0, 0.0, -1.0]), None);
        assert_eq!(
            pick(&m, [2.0, 2.0, 30.0], [0.0, 0.0, 1.0]),
            None,
            "behind the ray"
        );
    }

    #[test]
    fn pick_works_on_sides_without_faces() {
        let mut element = Element::cube([0.0; 3], [4.0; 3], "#0");
        element.faces.clear();
        let hit = pick(&model(vec![element]), [2.0, 2.0, 30.0], [0.0, 0.0, -1.0]);
        assert_eq!(hit.map(|h| h.direction), Some(Direction::South));
    }
}
