use glam::{Mat4, Vec3, Vec4};

const NEAR: f32 = 0.1;
const FAR: f32 = 2000.0;
const MIN_DISTANCE: f32 = 2.0;
const MAX_DISTANCE: f32 = 800.0;
const MAX_PITCH: f32 = 1.55;

/// An orbit camera: it looks at `target` from `distance` away at the given angles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    /// Rotation around the vertical axis, in radians.
    pub yaw: f32,
    /// Elevation above the horizon, in radians.
    pub pitch: f32,
    pub distance: f32,
    /// Vertical field of view, in radians.
    pub fov_y: f32,
}

impl Default for Camera {
    /// Frames a single block (0..16 on each axis) from the front-right.
    fn default() -> Self {
        Self {
            target: Vec3::splat(8.0),
            yaw: 35f32.to_radians(),
            pitch: 25f32.to_radians(),
            distance: 48.0,
            fov_y: 40f32.to_radians(),
        }
    }
}

impl Camera {
    pub fn eye(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        self.target + self.distance * Vec3::new(cos_pitch * sin_yaw, sin_pitch, cos_pitch * cos_yaw)
    }

    pub fn view(&self) -> Mat4 {
        // Right-handed look-at: the camera looks down its own -z axis.
        let eye = self.eye();
        let forward = (self.target - eye).normalize();
        let side = forward.cross(Vec3::Y).normalize();
        let up = side.cross(forward);
        Mat4::from_cols(
            Vec4::new(side.x, up.x, -forward.x, 0.0),
            Vec4::new(side.y, up.y, -forward.y, 0.0),
            Vec4::new(side.z, up.z, -forward.z, 0.0),
            Vec4::new(-side.dot(eye), -up.dot(eye), forward.dot(eye), 1.0),
        )
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        // Right-handed perspective projection with depth in 0..1, as wgpu expects.
        let h = 1.0 / (self.fov_y / 2.0).tan();
        let w = h / aspect.max(0.01);
        let range = FAR / (NEAR - FAR);
        let projection = Mat4::from_cols(
            Vec4::new(w, 0.0, 0.0, 0.0),
            Vec4::new(0.0, h, 0.0, 0.0),
            Vec4::new(0.0, 0.0, range, -1.0),
            Vec4::new(0.0, 0.0, range * NEAR, 0.0),
        );
        projection * self.view()
    }

    /// Rotates around the target; arguments are angle changes in radians.
    pub fn orbit(&mut self, yaw: f32, pitch: f32) {
        self.yaw += yaw;
        self.pitch = (self.pitch + pitch).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Multiplies the distance to the target (values below 1 zoom in).
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// Slides the target sideways/up. `right` and `up` are fractions of the viewport height.
    pub fn pan(&mut self, right: f32, up: f32) {
        let forward = (self.target - self.eye()).normalize();
        let right_axis = forward.cross(Vec3::Y).normalize();
        let up_axis = right_axis.cross(forward);
        let visible_height = 2.0 * self.distance * (self.fov_y / 2.0).tan();
        self.target += (right_axis * right + up_axis * up) * visible_height;
    }

    /// The world-space ray through a point in normalised device coordinates
    /// (`x`, `y` in -1..1 with +y up). Returns `(origin, direction)`.
    pub fn ray(&self, ndc: [f32; 2], aspect: f32) -> ([f32; 3], [f32; 3]) {
        let inverse = self.view_proj(aspect).inverse();
        let unproject = |z: f32| {
            let p = inverse * Vec4::new(ndc[0], ndc[1], z, 1.0);
            p.truncate() / p.w
        };
        let (near, far) = (unproject(0.0), unproject(1.0));
        (near.to_array(), (far - near).normalize().to_array())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_ray_points_at_target() {
        let camera = Camera::default();
        let (origin, direction) = camera.ray([0.0, 0.0], 1.5);
        let expected = (camera.target - camera.eye()).normalize();
        assert!(Vec3::from(direction).dot(expected) > 0.9999);
        assert!((Vec3::from(origin) - camera.eye()).length() < 0.2);
    }

    #[test]
    fn target_projects_to_screen_centre() {
        let camera = Camera::default();
        let clip = camera.view_proj(2.0) * camera.target.extend(1.0);
        assert!((clip.x / clip.w).abs() < 1e-4 && (clip.y / clip.w).abs() < 1e-4);
        let depth = clip.z / clip.w;
        assert!((0.0..1.0).contains(&depth));
    }

    #[test]
    fn orbit_zoom_and_pan_stay_sane() {
        let mut camera = Camera::default();
        camera.orbit(0.0, 10.0);
        assert!(camera.pitch <= MAX_PITCH);
        camera.zoom(0.0);
        assert_eq!(camera.distance, MIN_DISTANCE);

        let mut camera = Camera {
            yaw: 0.0,
            pitch: 0.0,
            ..Camera::default()
        };
        let before = camera.target;
        camera.pan(0.5, 0.0);
        // Looking down -z from +z, "right" on screen is +x.
        let moved = camera.target - before;
        assert!(
            moved.x > 0.0 && moved.y.abs() < 1e-4 && moved.z.abs() < 1e-4,
            "{moved:?}"
        );
    }
}
