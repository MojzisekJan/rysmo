//! Perspective calibration from two orthogonal vanishing points and a ground-plane length.
use rysmo_math::{DMat3, DVec2, DVec3, OrbitCamera};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    pub image: crate::ImageId,
    pub size: [u32; 2],
    /// Two lines for each of the X and Y direction families, in image fractions.
    pub guides: [[[DVec2; 2]; 2]; 2],
    pub origin: DVec2,
    /// Other endpoint of a measured ground-plane segment starting at the origin.
    pub reference: DVec2,
    pub reference_length: f64,
    pub flip_x: bool,
    pub flip_y: bool,
}
impl Setup {
    pub fn new(image: crate::ImageId, size: [u32; 2]) -> Self {
        Self {
            image,
            size,
            guides: [
                [[DVec2::new(0.1, 0.7), DVec2::new(0.8, 0.45)], [DVec2::new(0.2, 0.9), DVec2::new(0.85, 0.55)]],
                [[DVec2::new(0.2, 0.45), DVec2::new(0.9, 0.7)], [DVec2::new(0.15, 0.55), DVec2::new(0.8, 0.9)]],
            ],
            origin: DVec2::new(0.5, 0.8),
            reference: DVec2::new(0.7, 0.75),
            reference_length: 1.,
            flip_x: false,
            flip_y: false,
        }
    }
    pub fn valid(&self) -> bool {
        self.size.iter().all(|v| *v > 0 && *v <= 32768)
            && self.size[0] as u64 * self.size[1] as u64 <= 16_777_216
            && self
                .guides
                .iter()
                .flatten()
                .flatten()
                .chain([&self.origin, &self.reference])
                .all(|p| p.is_finite() && p.cmpge(DVec2::ZERO).all() && p.cmple(DVec2::ONE).all())
            && self.reference_length.is_finite()
            && (0.001..=1e6).contains(&self.reference_length)
    }
    /// Principal point is the image centre; square pixels, zero skew, no lens distortion.
    pub fn camera(&self) -> Result<OrbitCamera, String> {
        if !self.valid() {
            return Err("Invalid photo size, guide coordinates or reference length.".into());
        }
        let size = DVec2::new(self.size[0] as f64, self.size[1] as f64);
        let centre = size * 0.5;
        let vanishing = |family: [[DVec2; 2]; 2]| -> Result<DVec2, String> {
            let [[a, b], [c, d]] = family.map(|line| line.map(|p| p * size));
            let u = b - a;
            let v = d - c;
            let det = u.perp_dot(v);
            if u.length() < 2. || v.length() < 2. || det.abs() < u.length() * v.length() * 1e-5 {
                return Err("Guide lines must be distinct and converge to a finite vanishing point.".into());
            }
            Ok(a + u * ((c - a).perp_dot(v) / det))
        };
        let vx = vanishing(self.guides[0])? - centre;
        let vy = vanishing(self.guides[1])? - centre;
        let focal2 = -vx.dot(vy);
        if !focal2.is_finite() || focal2 < 1. {
            return Err("Guide directions do not define an orthogonal perspective. Align the red and green lines with perpendicular building edges.".into());
        }
        let focal = focal2.sqrt();
        let dx = DVec3::new(vx.x, -vx.y, -focal).normalize() * if self.flip_x { -1. } else { 1. };
        let mut dy = DVec3::new(vy.x, -vy.y, -focal).normalize() * if self.flip_y { -1. } else { 1. };
        dy = (dy - dx * dx.dot(dy)).normalize();
        let dz = dx.cross(dy).normalize();
        let rotation = DMat3::from_cols(dx, dy, dz);
        let inverse = rotation.transpose();
        let ray = |p: DVec2| {
            let p = p * size - centre;
            DVec3::new(p.x / focal, -p.y / focal, -1.)
        };
        let eye = inverse * (-ray(self.origin));
        // Mirrored direction choices solve the same image from underneath the ground plane.
        if eye.z <= 0. {
            return Err("These directions place the camera below the ground. Toggle Reverse X or Reverse Y.".into());
        }
        let direction = inverse * ray(self.reference);
        if direction.z.abs() < 1e-8 {
            return Err("Reference segment is on the horizon; choose a visible ground-plane length.".into());
        }
        let parameter = -eye.z / direction.z;
        if parameter <= 0. {
            return Err("Reference endpoint is behind the camera. Adjust the origin and reference marker.".into());
        }
        let endpoint = eye + direction * parameter;
        let distance = endpoint.length();
        if distance < 1e-8 {
            return Err("Reference length needs two distinct points.".into());
        }
        let eye = eye * (self.reference_length / distance);
        let forward = inverse * DVec3::NEG_Z;
        let desired_up = inverse * DVec3::Y;
        let backward = -forward;
        let camera_distance = eye.length().max(0.01);
        let mut camera = OrbitCamera {
            target: eye + forward * camera_distance,
            distance: camera_distance,
            yaw: backward.y.atan2(backward.x),
            pitch: backward.z.asin(),
            fov_y: 2. * (size.y / (2. * focal)).atan(),
            parallel: false,
            eye_base_z: 0.,
            roll: 0.,
        };
        let up = camera.up();
        camera.roll = forward.dot(up.cross(desired_up)).atan2(up.dot(desired_up));
        if !camera.valid() {
            return Err("Calibration produced an invalid camera.".into());
        }
        Ok(camera)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_photo_recovers_projection_origin_and_metric_scale_with_roll() {
        let source =
            OrbitCamera { target: DVec3::ZERO, distance: 9., yaw: -0.8, pitch: 0.45, roll: 0.13, ..Default::default() };
        let size = DVec2::new(1200., 800.);
        let view = source.view(size);
        let project = |p: DVec3| view.project(p).unwrap() / size;
        let mut setup = Setup::new(crate::ImageId(1), [1200, 800]);
        setup.guides = [
            [[project(DVec3::ZERO), project(DVec3::X)], [project(DVec3::Y), project(DVec3::X + DVec3::Y)]],
            [[project(DVec3::ZERO), project(DVec3::Y)], [project(DVec3::X), project(DVec3::X + DVec3::Y)]],
        ];
        setup.origin = project(DVec3::ZERO);
        setup.reference = project(DVec3::X * 2.);
        setup.reference_length = 2.;
        // Resolve the sign ambiguity of vanishing points using the image's axis directions.
        let mut solved = None;
        let mut rejected = 0;
        for x in [false, true] {
            for y in [false, true] {
                setup.flip_x = x;
                setup.flip_y = y;
                let Ok(camera) = setup.camera() else {
                    rejected += 1;
                    continue;
                };
                assert!(camera.eye().z > 0.);
                let p = camera.view(size).project(DVec3::X).unwrap() / size;
                if p.distance(project(DVec3::X)) < 1e-8 && camera.eye().distance(source.eye()) < 1e-7 {
                    solved = Some(camera);
                }
            }
        }
        let solved = solved.expect("one orientation recovers the source camera");
        // The two mirrored orientations would put the camera underground.
        assert_eq!(rejected, 2);
        for point in [DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z, DVec3::new(1., 2., 3.)] {
            assert!((solved.view(size).project(point).unwrap() - view.project(point).unwrap()).length() < 1e-6);
        }
    }
    #[test]
    fn coincident_guides_and_nonfinite_reference_reject() {
        let mut setup = Setup::new(crate::ImageId(1), [1000, 800]);
        setup.guides[0][1] = setup.guides[0][0];
        assert!(setup.camera().is_err());
        setup.reference_length = f64::NAN;
        assert!(!setup.valid());
    }
}

#[cfg(test)]
mod image_history_tests {
    use crate::{Command, Editor};
    #[test]
    fn replacing_and_removing_photos_prunes_unused_images_but_undo_restores_them() {
        let mut editor = Editor::new();
        let photo = |name: &str| Command::ImportPhoto {
            name: name.into(),
            mime: "image/png".into(),
            data: vec![1, 2, 3],
            size: [20, 20],
        };
        editor.apply(photo("First")).unwrap();
        let first = editor.doc().photo_match.as_ref().unwrap().image;
        editor.apply(photo("Second")).unwrap();
        assert_eq!(editor.doc().images.len(), 1);
        assert!(!editor.doc().images.contains_key(&first));
        editor.undo();
        assert_eq!(editor.doc().photo_match.as_ref().unwrap().image, first);
        assert!(editor.doc().images.contains_key(&first));
        editor.apply(Command::SetPhotoMatch { setup: None }).unwrap();
        assert!(editor.doc().images.is_empty());
        editor.undo();
        assert!(editor.doc().images.contains_key(&first));
    }
}
