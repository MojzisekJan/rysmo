//! Bounded planar corner rounding. Solid edges and branched corners are excluded.
use crate::{Curve, CurveKind, EdgeId, Element, Face, Mesh};
use rysmo_math::{DVec3, EPS};

#[derive(Clone, Debug)]
pub struct Fillet {
    pub points: Vec<DVec3>,
    pub centre: DVec3,
    pub normal: DVec3,
    pub radius: f64,
    pub sweep: f64,
    pub remainders: [[DVec3; 2]; 2],
    face: Option<Face>,
}

impl Mesh {
    /// Plan a fillet of two straight edges sharing an unbranched corner. A face
    /// corner must belong to the same convex face without holes.
    pub fn fillet_plan(&self, edges: [EdgeId; 2], radius: f64, segments: u32) -> Result<Fillet, String> {
        let fail = |s: &str| Err(s.to_string());
        if !radius.is_finite() || radius <= EPS || !(2..=512).contains(&segments) {
            return fail("Use a positive radius and 2-512 segments.");
        }
        if edges[0] == edges[1] {
            return fail("Select two different edges.");
        }
        let a = self.edge(edges[0]).ok_or("The first edge no longer exists.")?;
        let b = self.edge(edges[1]).ok_or("The second edge no longer exists.")?;
        if [a, b].iter().any(|e| !e.visible() || e.procedural || e.curve.is_some()) {
            return fail("Fillet needs visible straight edges outside a curve.");
        }
        let corner = a.v.iter().copied().find(|v| b.v.contains(v)).ok_or("The edges must share a corner.")?;
        if self.edges_of_vertex(corner).len() != 2 {
            return fail("The corner must have exactly two edges.");
        }
        let origin = self.pos(corner);
        let far = [
            self.pos(*a.v.iter().find(|v| **v != corner).unwrap()),
            self.pos(*b.v.iter().find(|v| **v != corner).unwrap()),
        ];
        let u = (far[0] - origin).try_normalize().ok_or("The first edge is too short.")?;
        let v = (far[1] - origin).try_normalize().ok_or("The second edge is too short.")?;
        let normal = u.cross(v).try_normalize().ok_or("Collinear edges cannot be rounded.")?;
        let theta = u.dot(v).clamp(-1., 1.).acos();
        let distance = radius / (theta * 0.5).tan();
        if !distance.is_finite() || distance <= EPS || far.iter().any(|p| distance >= p.distance(origin) - EPS) {
            return fail("The radius reaches beyond an adjacent edge. Use a smaller radius.");
        }
        let faces = self.faces_of_edge(edges[0]);
        if faces.len() > 1 || faces != self.faces_of_edge(edges[1]) {
            return fail("Use a wire corner or two boundary edges of the same face.");
        }
        let face = faces.first().and_then(|id| self.face(*id)).cloned();
        if let Some(f) = &face {
            if f.hidden || !f.holes.is_empty() {
                return fail("Hidden faces and faces with holes are not supported.");
            }
            let n = self.face_normal(faces[0]).ok_or("The face is degenerate.")?;
            let points = self.loop_points(&f.outer);
            for i in 0..points.len() {
                let x = points[(i + 1) % points.len()] - points[i];
                let y = points[(i + 2) % points.len()] - points[(i + 1) % points.len()];
                if x.cross(y).dot(n) < -EPS * x.length().max(1.) * y.length().max(1.) {
                    return fail("Face rounding currently requires a convex outline.");
                }
            }
        }
        let start = origin + u * distance;
        let end = origin + v * distance;
        let centre = origin + (u + v).normalize() * (radius / (theta * 0.5).sin());
        let radial = start - centre;
        // The minor arc bends away from the removed corner.
        let sweep = -(std::f64::consts::PI - theta);
        let side = normal.cross(radial);
        let mut points: Vec<_> = (0..=segments)
            .map(|i| {
                let angle = sweep * i as f64 / segments as f64;
                centre + radial * angle.cos() + side * angle.sin()
            })
            .collect();
        points[0] = start;
        points[segments as usize] = end;
        if !centre.is_finite() || points.iter().any(|p| !p.is_finite()) {
            return fail("The fillet is outside the supported coordinate range.");
        }
        Ok(Fillet {
            points,
            centre,
            normal,
            radius,
            sweep: sweep.to_degrees(),
            remainders: [[far[0], start], [end, far[1]]],
            face,
        })
    }

    /// Stage reconstruction before replacing the mesh, including edge and face styles.
    pub fn fillet(&mut self, edges: [EdgeId; 2], radius: f64, segments: u32) -> Result<Vec<Element>, String> {
        let plan = self.fillet_plan(edges, radius, segments)?;
        let styles = [self.edge(edges[0]).unwrap().clone(), self.edge(edges[1]).unwrap().clone()];
        let mut result = self.clone();
        let faces_before: std::collections::BTreeSet<_> = self.faces().map(|(id, _)| id).collect();
        result.erase(&edges.map(Element::Edge));
        for (line, style) in plan.remainders.iter().zip(styles) {
            let ids = result.add_lines(&[(line[0], line[1])], None).map_err(|e| e.to_string())?;
            for id in ids {
                if let Some(edge) = result.edges.get_mut(&id) {
                    edge.material = style.material;
                    edge.soft = style.soft;
                    edge.smooth = style.smooth;
                }
            }
        }
        let lines: Vec<_> = plan.points.windows(2).map(|p| (p[0], p[1])).collect();
        let members = result.add_lines(&lines, Some(plan.normal)).map_err(|e| e.to_string())?;
        if result.edges.len() != self.edges.len() + segments as usize
            || result.faces.len() != self.faces.len()
            || self.edges().any(|(id, edge)| !edges.contains(&id) && result.edge(id) != Some(edge))
        {
            return Err("The fillet intersects other geometry. The model was not changed.".into());
        }
        if let Some(style) = plan.face {
            let old_normal =
                rysmo_math::Plane::from_polygon(&self.loop_points(&style.outer)).ok_or("Invalid face plane.")?.normal;
            for (_, face) in result.faces.iter_mut().filter(|(id, _)| !faces_before.contains(id)) {
                let new_normal = rysmo_math::Plane::from_polygon(
                    &face.outer.iter().map(|v| result.vertices[v].pos).collect::<Vec<_>>(),
                )
                .ok_or("Invalid rounded face.")?
                .normal;
                if old_normal.dot(new_normal) < 0. {
                    face.outer.reverse();
                }
                face.material = style.material;
                face.back_material = style.back_material;
                face.uv = style.uv;
            }
        }
        result
            .register_curve(
                &members,
                Curve {
                    kind: CurveKind::Arc,
                    faceted: false,
                    centre: plan.centre,
                    normal: plan.normal,
                    start: (plan.points[0] - plan.centre).normalize(),
                    radius,
                    segments,
                    sweep: plan.sweep,
                },
            )
            .map_err(|e| e.to_string())?;
        result.validate().map_err(|e| e.to_string())?;
        let mut created: Vec<_> = members.into_iter().map(Element::Edge).collect();
        created.extend(result.faces().filter(|(id, _)| !faces_before.contains(id)).map(|(id, _)| Element::Face(id)));
        *self = result;
        Ok(created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn corner() -> (Mesh, [EdgeId; 2]) {
        let mut mesh = Mesh::new();
        let edges = mesh.add_lines(&[(DVec3::X * 4., DVec3::ZERO), (DVec3::ZERO, DVec3::Y * 4.)], None).unwrap();
        (mesh, [edges[0], edges[1]])
    }
    #[test]
    fn wire_fillet_has_exact_radius_tangencies_and_no_corner() {
        let (mut mesh, edges) = corner();
        let plan = mesh.fillet_plan(edges, 1., 12).unwrap();
        assert!(plan.points[0].distance(DVec3::X) < 1e-8);
        assert!(plan.points[12].distance(DVec3::Y) < 1e-8);
        assert!(plan.centre.distance(DVec3::new(1., 1., 0.)) < 1e-8);
        assert!(plan.points.iter().all(|p| (p.distance(plan.centre) - 1.).abs() < 1e-8));
        assert!((plan.points[0] - plan.centre).dot(DVec3::X).abs() < 1e-8);
        assert!((plan.points[12] - plan.centre).dot(DVec3::Y).abs() < 1e-8);
        mesh.fillet(edges, 1., 12).unwrap();
        assert!(mesh.find_vertex(DVec3::ZERO).is_none());
        assert_eq!(mesh.edges().count(), 14);
        assert_eq!(mesh.curves().count(), 1);
        assert_eq!(mesh.face_count(), 0);
        mesh.validate().unwrap();
    }
    #[test]
    fn convex_face_preserves_facing_paint_and_placement() {
        let mut mesh = Mesh::new();
        let p = [DVec3::ZERO, DVec3::X * 4., DVec3::new(4., 4., 0.), DVec3::Y * 4.];
        mesh.add_lines(&(0..4).map(|i| (p[i], p[(i + 1) % 4])).collect::<Vec<_>>(), Some(DVec3::Z)).unwrap();
        let face = mesh.faces().next().unwrap().0;
        let old_normal = mesh.face_normal(face).unwrap();
        mesh.face_mut(face).unwrap().material = Some(crate::MaterialId(12));
        mesh.face_mut(face).unwrap().back_material = Some(crate::MaterialId(13));
        let placement =
            crate::TexturePlacement { offset: [0.2, 0.3], rotation: 30., scale: [2., 3.], ..Default::default() };
        mesh.face_mut(face).unwrap().uv = Some(placement);
        let vertex = mesh.find_vertex(DVec3::ZERO).unwrap();
        let edges = mesh.edges_of_vertex(vertex);
        mesh.fillet([edges[0], edges[1]], 1., 12).unwrap();
        assert_eq!(mesh.face_count(), 1);
        let (face, data) = mesh.faces().next().unwrap();
        assert_eq!(data.material, Some(crate::MaterialId(12)));
        assert_eq!(data.back_material, Some(crate::MaterialId(13)));
        assert_eq!(data.uv, Some(placement));
        assert!(mesh.face_normal(face).unwrap().dot(old_normal) > 0.999);
        assert!((mesh.face_area(face) - (15. + std::f64::consts::FRAC_PI_4)).abs() < 0.01);
    }
    #[test]
    fn rejected_corner_and_collisions_leave_mesh_unchanged() {
        let (mut mesh, edges) = corner();
        for radius in [-1., 0., 4., 10., f64::NAN] {
            let before = mesh.clone();
            assert!(mesh.fillet(edges, radius, 12).is_err());
            assert_eq!(mesh, before);
        }
        mesh.add_lines(&[(DVec3::new(-1., 0.5, 0.), DVec3::new(2., 0.5, 0.))], None).unwrap();
        let before = mesh.clone();
        assert!(mesh.fillet(edges, 1., 12).is_err());
        assert_eq!(mesh, before);
    }
    #[test]
    fn tilted_far_origin_corner_and_reversed_order() {
        let (source, edges) = corner();
        for reversed in [false, true] {
            let mut mesh = source.clone();
            let rotation = rysmo_math::DQuat::from_rotation_x(0.6);
            let shift = DVec3::new(1e6, -1e6, 5e5);
            mesh.map_all(|p| rotation * p + shift);
            let chosen = if reversed { [edges[1], edges[0]] } else { edges };
            let plan = mesh.fillet_plan(chosen, 0.5, 16).unwrap();
            assert!(plan.centre.distance(rotation * DVec3::new(0.5, 0.5, 0.) + shift) < 1e-8);
            mesh.fillet(chosen, 0.5, 16).unwrap();
            mesh.validate().unwrap();
        }
    }
}
