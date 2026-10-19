//! World-space camera walking through visible document occurrences.
use crate::Document;
use rysmo_math::{DVec3, OrbitCamera, Ray};

#[derive(Clone, Copy, Debug)]
pub struct SurfaceHit {
    pub point: DVec3,
    pub normal: DVec3,
    pub distance: f64,
}

/// Continuous separating-axis test for a moving axis-aligned camera body and a
/// triangle. Includes edge axes, so polygon edges and openings are collidable.
fn swept_box_triangle(centre: DVec3, half: DVec3, movement: DVec3, triangle: [DVec3; 3]) -> Option<f64> {
    let edges = [triangle[1] - triangle[0], triangle[2] - triangle[1], triangle[0] - triangle[2]];
    let world = [DVec3::X, DVec3::Y, DVec3::Z];
    let axes = [
        world[0],
        world[1],
        world[2],
        edges[0].cross(edges[1]),
        edges[0].cross(world[0]),
        edges[0].cross(world[1]),
        edges[0].cross(world[2]),
        edges[1].cross(world[0]),
        edges[1].cross(world[1]),
        edges[1].cross(world[2]),
        edges[2].cross(world[0]),
        edges[2].cross(world[1]),
        edges[2].cross(world[2]),
    ];
    let mut enter = f64::NEG_INFINITY;
    let mut leave = f64::INFINITY;
    for axis in axes {
        if axis.length_squared() < 1e-20 {
            continue;
        }
        let axis = axis.normalize();
        let projections = triangle.map(|p| p.dot(axis));
        let min = projections.into_iter().fold(f64::INFINITY, f64::min);
        let max = projections.into_iter().fold(f64::NEG_INFINITY, f64::max);
        let radius = half.dot(axis.abs());
        let at = centre.dot(axis);
        let velocity = movement.dot(axis);
        if velocity.abs() < 1e-12 {
            if at + radius < min - 1e-9 || at - radius > max + 1e-9 {
                return None;
            }
        } else {
            let a = (min - at - radius) / velocity;
            let b = (max - at + radius) / velocity;
            enter = enter.max(a.min(b));
            leave = leave.min(a.max(b));
            if enter > leave + 1e-9 {
                return None;
            }
        }
    }
    // Permit departure from an initial overlap, such as a camera placed on a face.
    (enter >= -1e-8 && enter <= 1.0 && leave > 1e-8).then_some(enter.max(0.0))
}

impl Document {
    /// Nearest visible face, including transformed groups and components.
    pub fn ray_surface(&self, ray: Ray) -> Option<SurfaceHit> {
        if !ray.origin.is_finite() || !ray.dir.is_finite() || ray.dir.length_squared() < 1e-20 {
            return None;
        }
        let ray = Ray::new(ray.origin, ray.dir);
        self.occurrences(&[])
            .into_iter()
            .filter_map(|occ| {
                let mesh = self.effective_mesh(occ.definition);
                let inverse = occ.transform.inverse();
                if !inverse.is_finite() {
                    return None;
                }
                let local = Ray::new(inverse.transform_point3(ray.origin), inverse.transform_vector3(ray.dir));
                let clips = self.section_planes(&occ.path);
                mesh.faces()
                    .filter(|(_, face)| !face.hidden)
                    .filter_map(|(face, _)| {
                        let plane = mesh.face_plane(face)?;
                        let depth = plane.intersect_ray(&local)?;
                        let local_point = local.at(depth);
                        if !mesh.face_contains(face, local_point) {
                            return None;
                        }
                        let point = occ.transform.transform_point3(local_point);
                        if clips.iter().any(|plane| plane.normal.dot(point - plane.origin) > 1e-9) {
                            return None;
                        }
                        let distance = (point - ray.origin).dot(ray.dir);
                        let normal = inverse.transpose().transform_vector3(plane.normal).normalize();
                        (distance >= 0. && distance.is_finite()).then_some(SurfaceHit { point, normal, distance })
                    })
                    .min_by(|a, b| a.distance.total_cmp(&b.distance))
            })
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
    }

    /// Applies a walk displacement. Collision checks continuously sweep the camera's body against
    /// triangulated faces; short steps also sample the ground under the body.
    /// Ground within a step height follows floors and ramps. Unsupported space
    /// keeps the current elevation, as camera navigation does not simulate gravity.
    pub fn walk_camera(&self, camera: &mut OrbitCamera, delta: DVec3, bypass: bool, vertical: bool) {
        if !delta.is_finite() || delta.length_squared() < 1e-20 {
            return;
        }
        if bypass {
            camera.target += delta;
            return;
        }
        let triangles: Vec<_> = self
            .occurrences(&[])
            .into_iter()
            .flat_map(|occ| {
                let Some(_def) = self.def(occ.definition) else {
                    return Vec::new();
                };
                let mesh = self.effective_mesh(occ.definition);
                mesh.faces()
                    .filter(|(_, f)| !f.hidden)
                    .flat_map(|(id, _)| rysmo_mesh::triangulate_face(&mesh, id))
                    .map(|triangle| triangle.map(|id| occ.transform.transform_point3(mesh.pos(id))))
                    .collect::<Vec<_>>()
            })
            .collect();
        let height = camera.eye().z - camera.eye_base_z;
        let steps = (delta.length() / 0.08).ceil().clamp(1.0, 512.0) as usize;
        let step = delta / steps as f64;
        for _ in 0..steps {
            let eye = camera.eye();
            let movement = step;
            // Resolve each component so the camera can slide along a wall.
            for axis in 0..3 {
                let mut component = DVec3::ZERO;
                component[axis] = movement[axis];
                if component.length_squared() < 1e-20 {
                    continue;
                }
                let body_height = height.abs().max(0.3);
                let half = DVec3::new(0.18, 0.18, (body_height + 0.03) * 0.5);
                let centre = camera.eye() + DVec3::Z * (0.165 - body_height * 0.5);
                let fraction = triangles
                    .iter()
                    .filter_map(|triangle| swept_box_triangle(centre, half, component, *triangle))
                    .fold(1.0, f64::min);
                let fraction = if fraction < 1.0 { (fraction - 1e-7 / component.length()).max(0.0) } else { 1.0 };
                camera.target += component * fraction;
            }
            if !vertical && height > 0.0 {
                let feet = camera.eye() - DVec3::Z * height;
                if let Some(hit) = self.ray_surface(Ray::new(feet + DVec3::Z * 0.35, -DVec3::Z)) {
                    if hit.normal.z.abs() > 0.5 && hit.distance <= 0.7 {
                        let elevation = hit.point.z - feet.z;
                        // Following ground must not lift the body into a ceiling.
                        let body_height = height.abs().max(0.3);
                        let half = DVec3::new(0.18, 0.18, (body_height + 0.03) * 0.5);
                        let centre = camera.eye() + DVec3::Z * (0.165 - body_height * 0.5);
                        let clear = triangles.iter().all(|triangle| {
                            swept_box_triangle(centre, half, DVec3::Z * elevation, *triangle)
                                .is_none_or(|fraction| fraction >= 1.0)
                        });
                        if clear {
                            camera.target.z += elevation;
                            camera.eye_base_z = hit.point.z;
                        }
                    }
                }
            }
            if camera.eye().distance_squared(eye) < 1e-20 {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn polygon(doc: &mut Document, points: &[DVec3]) {
        let lines: Vec<_> = (0..points.len()).map(|i| (points[i], points[(i + 1) % points.len()])).collect();
        doc.root.mesh.add_lines(&lines, None).unwrap();
    }
    fn room() -> Document {
        let mut doc = Document::new();
        polygon(
            &mut doc,
            &[DVec3::new(-5., -5., 0.), DVec3::new(5., -5., 0.), DVec3::new(5., 5., 0.), DVec3::new(-5., 5., 0.)],
        );
        polygon(
            &mut doc,
            &[DVec3::new(1., -5., 0.), DVec3::new(1., 5., 0.), DVec3::new(1., 5., 3.), DVec3::new(1., -5., 3.)],
        );
        doc
    }
    #[test]
    fn walk_stops_at_wall_slides_and_alt_passes_through() {
        let doc = room();
        let mut camera = OrbitCamera::default();
        camera.place_eye(DVec3::new(0., 0., 1.7), None);
        doc.walk_camera(&mut camera, DVec3::new(2., 1., 0.), false, false);
        assert!(camera.eye().x <= 0.821);
        assert!((camera.eye().y - 1.).abs() < 1e-8);
        assert!((camera.eye().z - 1.7).abs() < 1e-8);
        doc.walk_camera(&mut camera, DVec3::X * 2., true, false);
        assert!(camera.eye().x > 2.);
    }
    #[test]
    fn transformed_group_hit_uses_world_distance_and_normal() {
        let mut doc = Document::new();
        polygon(
            &mut doc,
            &[DVec3::new(-2., -2., -2.), DVec3::new(-2., 2., -2.), DVec3::new(2., 2., 2.), DVec3::new(2., -2., 2.)],
        );
        let mut editor = crate::Editor::with_document(doc);
        let elements = editor
            .mesh()
            .edges()
            .map(|(id, _)| crate::Element::Edge(id))
            .chain(editor.mesh().faces().map(|(id, _)| crate::Element::Face(id)))
            .collect();
        let out = editor.apply(crate::Command::MakeGroup { elements }).unwrap();
        let crate::Element::Instance(id) = out.created[0] else {
            panic!("group");
        };
        let mut doc = editor.doc().clone();
        let transform = rysmo_math::DMat4::from_translation(DVec3::new(4., 2., 3.))
            * rysmo_math::DMat4::from_rotation_y(0.4)
            * rysmo_math::DMat4::from_scale(DVec3::new(2., 3., 0.5));
        doc.root.instances.get_mut(&id).unwrap().transform = transform;
        let plane = doc
            .def(Some(doc.root.instances[&id].definition))
            .unwrap()
            .mesh
            .faces()
            .next()
            .map(|(f, _)| doc.def(Some(doc.root.instances[&id].definition)).unwrap().mesh.face_plane(f).unwrap())
            .unwrap();
        let point = transform.transform_point3(plane.normal * plane.d);
        let normal = transform.inverse().transpose().transform_vector3(plane.normal).normalize();
        let hit = doc.ray_surface(Ray::new(point + normal * 2., -normal)).unwrap();
        assert!((hit.distance - 2.).abs() < 1e-8);
        assert!(hit.point.distance(point) < 1e-8);
        assert!(hit.normal.dot(normal).abs() > 1. - 1e-8);
    }
    #[test]
    fn walking_follows_a_ramp_without_changing_eye_height() {
        let mut doc = Document::new();
        polygon(
            &mut doc,
            &[DVec3::new(-2., -2., -0.5), DVec3::new(2., -2., 0.5), DVec3::new(2., 2., 0.5), DVec3::new(-2., 2., -0.5)],
        );
        let mut camera = OrbitCamera::default();
        camera.place_eye(DVec3::new(0., 0., 1.7), None);
        doc.walk_camera(&mut camera, DVec3::X, false, false);
        assert!((camera.eye().x - 1.).abs() < 1e-8);
        assert!((camera.eye_base_z - 0.25).abs() < 1e-8);
        assert!((camera.eye().z - camera.eye_base_z - 1.7).abs() < 1e-8);
    }
    #[test]
    fn low_beam_between_former_ray_samples_blocks_walking() {
        let mut doc = Document::new();
        polygon(
            &mut doc,
            &[DVec3::new(1., -1., 1.1), DVec3::new(1., 1., 1.1), DVec3::new(1., 1., 1.2), DVec3::new(1., -1., 1.2)],
        );
        let mut camera = OrbitCamera::default();
        camera.place_eye(DVec3::new(0., 0., 1.7), None);
        doc.walk_camera(&mut camera, DVec3::X * 2., false, false);
        assert!((camera.eye().x - 0.82).abs() < 1e-5);
    }
    #[test]
    fn doorway_clearance_and_concave_corner() {
        for (width, passes) in [(0.3, false), (0.6, true)] {
            let mut doc = Document::new();
            for (a, b) in [(-2., -width * 0.5), (width * 0.5, 2.)] {
                polygon(
                    &mut doc,
                    &[DVec3::new(1., a, 0.), DVec3::new(1., b, 0.), DVec3::new(1., b, 3.), DVec3::new(1., a, 3.)],
                );
            }
            let mut camera = OrbitCamera::default();
            camera.place_eye(DVec3::new(0., 0., 1.7), None);
            doc.walk_camera(&mut camera, DVec3::X * 2., false, false);
            assert_eq!(camera.eye().x > 1., passes);
        }
        let mut doc = room();
        polygon(
            &mut doc,
            &[DVec3::new(-5., 1., 0.), DVec3::new(5., 1., 0.), DVec3::new(5., 1., 3.), DVec3::new(-5., 1., 3.)],
        );
        let mut camera = OrbitCamera::default();
        camera.place_eye(DVec3::new(0., 0., 1.7), None);
        doc.walk_camera(&mut camera, DVec3::new(2., 2., 0.), false, false);
        assert!(camera.eye().x < 0.821 && camera.eye().y < 0.821);
        doc.walk_camera(&mut camera, DVec3::new(-1., -1., 0.), false, false);
        assert!(camera.eye().x < 0. && camera.eye().y < 0.);
    }
    #[test]
    fn vertical_motion_stops_above_floor_and_invalid_input_is_ignored() {
        let doc = room();
        let mut camera = OrbitCamera::default();
        camera.place_eye(DVec3::new(0., 0., 1.7), None);
        doc.walk_camera(&mut camera, -DVec3::Z * 3., false, true);
        assert!(camera.eye().z > 0.3);
        let before = camera;
        doc.walk_camera(&mut camera, DVec3::splat(f64::NAN), false, false);
        assert_eq!(camera, before);
    }
}

#[cfg(test)]
mod visible_surface_tests {
    use crate::{Command, Editor};
    use rysmo_math::{DVec3, Ray};
    #[test]
    fn surface_query_ignores_hidden_and_section_clipped_front_faces() {
        let mut editor = Editor::new();
        for z in [0., 1.] {
            editor
                .apply(Command::DrawRectangle {
                    corner: DVec3::Z * z,
                    u: DVec3::X * 2.,
                    v: DVec3::Y * 2.,
                    facing: Some(DVec3::Z),
                })
                .unwrap();
        }
        let ray = Ray::new(DVec3::new(1., 1., 3.), -DVec3::Z);
        assert!((editor.doc().ray_surface(ray).unwrap().point.z - 1.).abs() < 1e-9);
        let mut doc = editor.doc().clone();
        doc.root.sections.planes.insert(
            999,
            crate::sections::SectionPlane { name: "Cut".into(), origin: DVec3::Z * 0.5, normal: DVec3::Z },
        );
        doc.root.sections.active = Some(999);
        assert!(doc.ray_surface(ray).unwrap().point.z.abs() < 1e-9);
        let face = doc
            .root
            .mesh
            .faces()
            .find(|(id, _)| doc.root.mesh.face_plane(*id).unwrap().distance(DVec3::ZERO).abs() < 1e-9)
            .unwrap()
            .0;
        doc.root.mesh.face_mut(face).unwrap().hidden = true;
        assert!(doc.ray_surface(ray).is_none());
    }
}
