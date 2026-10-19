//! The orbit camera and the screen projection derived from it.
//!
//! The core owns the camera so that inference and picking can be tested without a
//! window, and so that the viewport draws exactly what the core measures against.

use glam::{DMat4, DVec2, DVec3, DVec4};

use crate::Ray;

/// A camera orbiting a target point, Z up.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OrbitCamera {
    pub target: DVec3,
    pub distance: f64,
    /// Angle around Z, radians, 0 looking from +X.
    pub yaw: f64,
    /// Angle above the XY plane, radians.
    pub pitch: f64,
    /// Vertical field of view, radians.
    pub fov_y: f64,
    #[serde(default)]
    pub parallel: bool,
    /// World elevation used as the reference for eye height.
    #[serde(default)]
    pub eye_base_z: f64,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        OrbitCamera {
            target: DVec3::new(1.0, 1.0, 0.5),
            distance: 9.0,
            yaw: -1.1,
            pitch: 0.5,
            fov_y: 35f64.to_radians(),
            parallel: false,
            eye_base_z: 0.0,
        }
    }
}

impl OrbitCamera {
    pub fn eye(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + DVec3::new(cp * cy, cp * sy, sp) * self.distance
    }

    /// Stable up direction, including exact top and bottom views.
    pub fn up(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        DVec3::new(-sp * cy, -sp * sy, cp)
    }

    pub fn set_standard_view(&mut self, view: StandardView) {
        let quarter = std::f64::consts::FRAC_PI_2;
        (self.yaw, self.pitch) = match view {
            StandardView::Top => (-quarter, quarter),
            StandardView::Bottom => (-quarter, -quarter),
            StandardView::Front => (-quarter, 0.0),
            StandardView::Back => (quarter, 0.0),
            StandardView::Left => (std::f64::consts::PI, 0.0),
            StandardView::Right => (0.0, 0.0),
            StandardView::Iso => (-std::f64::consts::FRAC_PI_4, (1.0 / 2.0_f64.sqrt()).atan()),
        };
    }

    /// The projection for a viewport of `size` pixels.
    pub fn view(&self, size: DVec2) -> View {
        let eye = self.eye();
        let near = (self.distance * 0.001).max(0.001);
        let far = self.distance * 1000.0 + 1000.0;
        let aspect = (size.x / size.y.max(1.0)).max(1e-3);
        let height = 2.0 * self.distance * (self.fov_y * 0.5).tan();
        let proj = if self.parallel {
            glam::dcamera::rh::proj::directx::orthographic(
                -height * aspect * 0.5,
                height * aspect * 0.5,
                -height * 0.5,
                height * 0.5,
                near,
                far,
            )
        } else {
            glam::dcamera::rh::proj::directx::perspective(self.fov_y, aspect, near, far)
        };
        View {
            eye,
            view: glam::dcamera::rh::view::look_at_mat4(eye, self.target, self.up()),
            proj,
            size,
            offset: DVec2::ZERO,
        }
    }

    /// Turns the camera by a drag of `delta` pixels.
    pub fn orbit(&mut self, delta: DVec2) {
        self.yaw -= delta.x * 0.008;
        self.pitch = (self.pitch + delta.y * 0.008).clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
    }

    /// Rotates around a picked point without shifting its screen position.
    pub fn orbit_about(&mut self, pivot: DVec3, delta: DVec2) {
        if !pivot.is_finite() || !delta.is_finite() || delta.length_squared() < 1e-20 {
            return;
        }
        let local = self.view(DVec2::ONE).view.transform_vector3(self.target - pivot);
        self.orbit(delta);
        let rotation = self.view(DVec2::ONE).view.inverse();
        self.target = pivot + rotation.transform_vector3(local);
    }

    pub fn look_around(&mut self, delta: DVec2) {
        if !delta.is_finite() || delta.length_squared() < 1e-20 {
            return;
        }
        let eye = self.eye();
        self.orbit(delta);
        self.target -= self.eye() - eye;
    }

    pub fn place_eye(&mut self, eye: DVec3, target: Option<DVec3>) -> bool {
        if !eye.is_finite() {
            return false;
        }
        let forward = if let Some(target) = target {
            let d = target - eye;
            if d.length() < 1e-6 || !d.is_finite() {
                return false;
            }
            d.normalize()
        } else {
            DVec3::new(-self.yaw.cos(), -self.yaw.sin(), 0.0)
        };
        let backward = -forward;
        self.yaw = backward.y.atan2(backward.x);
        self.pitch = backward.z.atan2(backward.truncate().length());
        self.distance = 1.0;
        self.target = eye + forward;
        self.parallel = false;
        true
    }

    pub fn set_eye_height(&mut self, base_z: f64, height: f64) -> bool {
        if !base_z.is_finite() || !height.is_finite() {
            return false;
        }
        self.target.z += base_z + height - self.eye().z;
        self.eye_base_z = base_z;
        true
    }

    /// Slides the camera so the target follows a drag of `delta` pixels.
    pub fn pan(&mut self, delta: DVec2, viewport: DVec2) {
        let view = self.view(viewport);
        let px = view.pixel_size_at(self.target);
        let inv = view.view.inverse();
        let right = inv.transform_vector3(DVec3::X);
        let up = inv.transform_vector3(DVec3::Y);
        self.target += (-right * delta.x + up * delta.y) * px;
    }

    /// Zooms by `factor` (below 1 moves closer) towards the point under `cursor`.
    pub fn zoom_at(&mut self, factor: f64, cursor: DVec2, viewport: DVec2) {
        let factor = factor.clamp(0.2, 5.0);
        let view = self.view(viewport);
        let ray = view.ray(cursor);
        // The point under the cursor at the target's depth.
        let forward = (self.target - view.eye).normalize();
        let depth = (self.target - ray.origin).dot(forward) / ray.dir.dot(forward).max(1e-3);
        let q = ray.at(depth);
        self.target += (q - self.target) * (1.0 - factor);
        self.distance = (self.distance * factor).clamp(0.01, 1e6);
    }

    /// Focal length using a 36 mm reference image span.
    pub fn focal_length(&self) -> f64 {
        18.0 / (self.fov_y * 0.5).tan()
    }

    /// Zoom's precise lens input; returns true when the value was a focal length.
    pub fn set_zoom_value(&mut self, text: &str) -> Result<bool, String> {
        if self.parallel {
            return Err("Switch to perspective to set field of view or focal length.".into());
        }
        let lower = text.trim().to_ascii_lowercase();
        let focal = lower.ends_with("mm");
        let number = if focal {
            lower.strip_suffix("mm").unwrap()
        } else {
            lower.strip_suffix("deg").or_else(|| lower.strip_suffix('°')).unwrap_or(&lower)
        };
        let value = number
            .trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or("Enter degrees or a focal length ending in mm.")?;
        if focal {
            if !(10.0..=2063.0).contains(&value) {
                return Err("Focal length must be 10 to 2063 mm.".into());
            }
            self.fov_y = 2.0 * (18.0 / value).atan();
        } else {
            if !(1.0..=120.0).contains(&value) {
                return Err("Field of view must be 1 to 120 degrees.".into());
            }
            self.fov_y = value.to_radians();
        }
        Ok(focal)
    }

    /// Frames a dragged screen rectangle at the target depth, preserving direction.
    pub fn zoom_window(&mut self, a: DVec2, b: DVec2, viewport: DVec2) -> bool {
        if !a.is_finite() || !b.is_finite() || !viewport.is_finite() || viewport.min_element() < 1.0 {
            return false;
        }
        let lo = a.min(b).clamp(DVec2::ZERO, viewport);
        let hi = a.max(b).clamp(DVec2::ZERO, viewport);
        let size = hi - lo;
        if size.min_element() < 4.0 {
            return false;
        }
        let fraction = (size / viewport).max_element();
        let ray = self.view(viewport).ray((lo + hi) * 0.5);
        let forward = (self.target - self.eye()).normalize();
        let den = ray.dir.dot(forward);
        if den <= 1e-9 {
            return false;
        }
        let target = ray.at((self.target - ray.origin).dot(forward) / den);
        if !target.is_finite() {
            return false;
        }
        self.target = target;
        self.distance = (self.distance * fraction).clamp(0.01, 1e6);
        true
    }

    /// Frames a box so all of it is visible, keeping the viewing direction.
    pub fn frame(&mut self, min: DVec3, max: DVec3) {
        let centre = (min + max) * 0.5;
        let radius = ((max - min).length() * 0.5).max(0.5);
        self.target = centre;
        self.distance = radius / (self.fov_y * 0.5).sin() * 1.1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandardView {
    Top,
    Bottom,
    Front,
    Back,
    Left,
    Right,
    Iso,
}

/// A camera's projection for one viewport size, in pixels with the origin top left.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct View {
    pub eye: DVec3,
    pub view: DMat4,
    pub proj: DMat4,
    pub size: DVec2,
    /// Origin of the drawing canvas in window coordinates.
    pub offset: DVec2,
}

impl View {
    /// The same camera seen from coordinates that `model` maps to world coordinates
    /// (inside a group or component being edited).
    pub fn local(&self, model: DMat4) -> View {
        View {
            eye: model.inverse().transform_point3(self.eye),
            view: self.view * model,
            proj: self.proj,
            size: self.size,
            offset: self.offset,
        }
    }

    /// The pixel a point projects to, or `None` behind the camera.
    pub fn project(&self, p: DVec3) -> Option<DVec2> {
        let clip = self.proj * self.view * DVec4::new(p.x, p.y, p.z, 1.0);
        if clip.w <= 1e-9 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(self.offset + DVec2::new((ndc.x + 1.0) * 0.5 * self.size.x, (1.0 - ndc.y) * 0.5 * self.size.y))
    }

    /// The ray from the eye through a pixel.
    pub fn ray(&self, px: DVec2) -> Ray {
        let px = px - self.offset;
        let ndc = DVec2::new(px.x / self.size.x * 2.0 - 1.0, 1.0 - px.y / self.size.y * 2.0);
        let inv = (self.proj * self.view).inverse();
        let far = inv * DVec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let far = far.truncate() / far.w;
        if self.proj.w_axis.w.abs() > 0.5 {
            let near = inv * DVec4::new(ndc.x, ndc.y, 0.0, 1.0);
            let near = near.truncate() / near.w;
            Ray::new(near, far - near)
        } else {
            Ray::new(self.eye, far - self.eye)
        }
    }

    /// How long one pixel is in world units at the depth of `p`.
    pub fn pixel_size_at(&self, p: DVec3) -> f64 {
        let depth = (self.view.transform_point3(p).z).abs().max(1e-6);
        // proj.y_axis.y is 1 / tan(fov / 2).
        let depth = if self.proj.w_axis.w.abs() > 0.5 { 1.0 } else { depth };
        2.0 * depth / (self.proj.y_axis.y * self.size.y)
    }

    /// Projects the segment `a..b` to the screen, clipped to the near side of the eye.
    pub fn project_segment(&self, a: DVec3, b: DVec3) -> Option<(DVec2, DVec2)> {
        let za = self.view.transform_point3(a).z;
        let zb = self.view.transform_point3(b).z;
        let near = -1e-4;
        if za > near && zb > near {
            return None;
        }
        let clip = |p: DVec3, q: DVec3, zp: f64, zq: f64| {
            if zp <= near { p } else { p + (q - p) * ((zp - near) / (zp - zq)) }
        };
        let a2 = clip(a, b, za, zb);
        let b2 = clip(b, a, zb, za);
        Some((self.project(a2)?, self.project(b2)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_canvas_projects_and_picks_in_window_coordinates() {
        let camera = OrbitCamera::default();
        let mut view = camera.view(DVec2::new(800., 500.));
        view.offset = DVec2::new(40., 90.);
        assert!(view.project(camera.target).unwrap().distance(view.offset + view.size * 0.5) < 1e-8);
        let point = DVec3::new(0.3, 0.2, 0.4);
        let pixel = view.project(point).unwrap();
        let ray = view.ray(pixel);
        assert!((point - ray.origin).cross(ray.dir).length() < 1e-8);
        let transform = DMat4::from_translation(DVec3::new(2., 3., 4.));
        assert!(
            view.local(transform).project(transform.inverse().transform_point3(point)).unwrap().distance(pixel) < 1e-8
        );
    }
    #[test]
    fn target_projects_to_centre_and_ray_returns() {
        let cam = OrbitCamera::default();
        let size = DVec2::new(800.0, 600.0);
        let v = cam.view(size);
        let c = v.project(cam.target).unwrap();
        assert!((c - size * 0.5).length() < 1e-6);
        let p = DVec3::new(0.3, -0.2, 0.7);
        let px = v.project(p).unwrap();
        let ray = v.ray(px);
        let t = (p - ray.origin).dot(ray.dir);
        assert!((ray.at(t) - p).length() < 1e-6);
    }

    #[test]
    fn pixel_size_matches_projection() {
        let cam = OrbitCamera::default();
        let v = cam.view(DVec2::new(800.0, 600.0));
        let s = v.pixel_size_at(cam.target);
        let a = v.project(cam.target).unwrap();
        let inv = v.view.inverse();
        let right = inv.transform_vector3(DVec3::X);
        let b = v.project(cam.target + right * s * 10.0).unwrap();
        assert!(((b - a).length() - 10.0).abs() < 1e-3);
    }

    #[test]
    fn local_view_projects_like_the_world_view() {
        let cam = OrbitCamera::default();
        let v = cam.view(DVec2::new(800.0, 600.0));
        let model = DMat4::from_translation(DVec3::new(2.0, 1.0, 0.0)) * DMat4::from_rotation_z(0.7);
        let local = v.local(model);
        let p = DVec3::new(0.3, 0.2, 0.5);
        let a = local.project(p).unwrap();
        let b = v.project(model.transform_point3(p)).unwrap();
        assert!((a - b).length() < 1e-6);
        let ray = local.ray(a);
        let t = (p - ray.origin).dot(ray.dir);
        assert!((ray.at(t) - p).length() < 1e-6);
    }

    #[test]
    fn zoom_towards_centre_keeps_target() {
        let mut cam = OrbitCamera::default();
        let size = DVec2::new(800.0, 600.0);
        let t = cam.target;
        cam.zoom_at(0.5, size * 0.5, size);
        assert!((cam.target - t).length() < 1e-9);
        assert!((cam.distance - 4.5).abs() < 1e-9);
    }
}

#[cfg(test)]
mod parallel_tests {
    use super::*;

    #[test]
    fn standard_views_have_finite_projection_and_parallel_picking_matches() {
        let size = DVec2::new(800.0, 600.0);
        for direction in [
            StandardView::Top,
            StandardView::Bottom,
            StandardView::Front,
            StandardView::Back,
            StandardView::Left,
            StandardView::Right,
            StandardView::Iso,
        ] {
            let mut camera = OrbitCamera { parallel: true, ..Default::default() };
            camera.set_standard_view(direction);
            let view = camera.view(size);
            assert!(view.view.is_finite());
            assert!((view.project(camera.target).unwrap() - size * 0.5).length() < 1e-7);
            let right = view.view.inverse().transform_vector3(DVec3::X);
            let point = camera.target + right * 0.7;
            let pixel = view.project(point).unwrap();
            let ray = view.ray(pixel);
            let depth = (point - ray.origin).dot(ray.dir);
            assert!((ray.at(depth) - point).length() < 1e-7);
            let central_ray = view.ray(size * 0.5);
            assert!((central_ray.dir - ray.dir).length() < 1e-7);
            let behind = point + ray.dir * 10.0;
            assert!((view.project(behind).unwrap() - pixel).length() < 1e-7);
            assert!((view.pixel_size_at(behind) - view.pixel_size_at(point)).abs() < 1e-10);
        }
    }

    #[test]
    fn parallel_zoom_keeps_cursor_point_fixed_and_pan_uses_projection_scale() {
        let size = DVec2::new(800.0, 600.0);
        let mut camera = OrbitCamera { parallel: true, ..Default::default() };
        camera.set_standard_view(StandardView::Top);
        let point = camera.target + DVec3::X;
        let pixel = camera.view(size).project(point).unwrap();
        camera.zoom_at(0.5, pixel, size);
        assert!((camera.view(size).project(point).unwrap() - pixel).length() < 1e-7);
        let before = camera.view(size).project(point).unwrap();
        camera.pan(DVec2::new(40.0, 20.0), size);
        let after = camera.view(size).project(point).unwrap();
        assert!((after - before - DVec2::new(40.0, 20.0)).length() < 1e-7);
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;
    #[test]
    fn window_zoom_centres_and_fits_both_projections_at_target_depth() {
        let viewport = DVec2::new(800.0, 600.0);
        let a = DVec2::new(100.0, 160.0);
        let b = DVec2::new(500.0, 360.0);
        for parallel in [false, true] {
            let mut camera = OrbitCamera { parallel, ..Default::default() };
            let before = camera;
            let view = camera.view(viewport);
            let forward = (camera.target - camera.eye()).normalize();
            let world_at = |pixel| {
                let ray = view.ray(pixel);
                ray.at((before.target - ray.origin).dot(forward) / ray.dir.dot(forward))
            };
            let centre = world_at((a + b) * 0.5);
            let points = [world_at(a), world_at(b)];
            assert!(camera.zoom_window(a, b, viewport));
            assert!((camera.distance - before.distance * 0.5).abs() < 1e-7);
            assert_eq!((camera.yaw, camera.pitch, camera.fov_y), (before.yaw, before.pitch, before.fov_y));
            let view = camera.view(viewport);
            assert!((view.project(centre).unwrap() - viewport * 0.5).length() < 1e-6);
            for p in points {
                let screen = view.project(p).unwrap();
                assert!(screen.x >= -1e-6 && screen.x <= 800.0 + 1e-6 && screen.y >= -1e-6 && screen.y <= 600.0 + 1e-6);
            }
            let mut reversed = before;
            assert!(reversed.zoom_window(b, a, viewport));
            assert!((reversed.target - camera.target).length() < 1e-8);
            assert_eq!(reversed.distance, camera.distance);
        }
    }
    #[test]
    fn invalid_window_zoom_does_not_change_camera() {
        let mut camera = OrbitCamera::default();
        let before = camera;
        let viewport = DVec2::new(800.0, 600.0);
        assert!(!camera.zoom_window(DVec2::ZERO, DVec2::new(2.0, 2.0), viewport));
        assert!(!camera.zoom_window(DVec2::splat(f64::NAN), viewport, viewport));
        assert!(!camera.zoom_window(DVec2::ZERO, viewport, DVec2::ZERO));
        assert_eq!(camera, before);
    }
}

#[cfg(test)]
mod pivot_tests {
    use super::*;
    #[test]
    fn picked_orbit_point_stays_on_screen_and_at_constant_eye_distance() {
        let viewport = DVec2::new(800.0, 600.0);
        for parallel in [false, true] {
            let mut camera = OrbitCamera { parallel, ..Default::default() };
            let pivot = camera.target + DVec3::new(1.0, 0.5, -0.3);
            let pixel = camera.view(viewport).project(pivot).unwrap();
            let distance = camera.eye().distance(pivot);
            camera.orbit_about(pivot, DVec2::new(45.0, -20.0));
            assert!((camera.view(viewport).project(pivot).unwrap() - pixel).length() < 1e-6);
            assert!((camera.eye().distance(pivot) - distance).abs() < 1e-7);
        }
    }
    #[test]
    fn zero_orbit_preserves_exact_top_view() {
        let mut camera = OrbitCamera::default();
        camera.set_standard_view(StandardView::Top);
        let before = camera;
        camera.orbit_about(DVec3::ZERO, DVec2::ZERO);
        assert_eq!(camera, before);
        camera.orbit(DVec2::ZERO);
        assert_eq!(camera, before);
    }
}

#[cfg(test)]
mod lens_tests {
    use super::*;
    #[test]
    fn zoom_precise_angle_and_lens_keep_camera_position() {
        let mut camera = OrbitCamera::default();
        let eye = camera.eye();
        assert_eq!(camera.set_zoom_value("45 deg"), Ok(false));
        assert!((camera.fov_y.to_degrees() - 45.0).abs() < 1e-9);
        assert_eq!(camera.set_zoom_value("35 mm"), Ok(true));
        assert!((camera.focal_length() - 35.0).abs() < 1e-9);
        assert_eq!(camera.eye(), eye);
        let before = camera;
        assert!(camera.set_zoom_value("0deg").is_err());
        assert!(camera.set_zoom_value("NaNmm").is_err());
        assert_eq!(camera, before);
        camera.parallel = true;
        assert!(camera.set_zoom_value("45deg").is_err());
    }
}

#[cfg(test)]
mod position_tests {
    use super::*;
    #[test]
    fn look_around_turns_without_moving_eye() {
        let mut camera = OrbitCamera::default();
        let eye = camera.eye();
        let old_target = camera.target;
        camera.look_around(DVec2::new(30.0, -15.0));
        assert!((camera.eye() - eye).length() < 1e-7);
        assert!(camera.target.distance(old_target) > 0.1);
    }
    #[test]
    fn placement_aim_and_eye_height_keep_direction_consistent() {
        let mut camera = OrbitCamera { parallel: true, ..Default::default() };
        let eye = DVec3::new(2.0, 3.0, 1.6764);
        let target = DVec3::new(6.0, 5.0, 2.0);
        assert!(camera.place_eye(eye, Some(target)));
        assert!(!camera.parallel);
        assert!((camera.eye() - eye).length() < 1e-7);
        assert!((camera.target - camera.eye()).normalize().cross((target - eye).normalize()).length() < 1e-7);
        let direction = (camera.target - camera.eye()).normalize();
        assert!(camera.set_eye_height(1.0, 2.0));
        assert!((camera.eye().z - 3.0).abs() < 1e-7);
        assert!((camera.target - camera.eye()).normalize().cross(direction).length() < 1e-7);
        let before = camera;
        assert!(!camera.place_eye(camera.eye(), Some(camera.eye())));
        assert_eq!(camera, before);
        assert!(!camera.set_eye_height(0.0, f64::NAN));
        assert_eq!(camera, before);
    }
}
