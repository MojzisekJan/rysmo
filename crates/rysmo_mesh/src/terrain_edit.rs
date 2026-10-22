//! Local topology edits for triangulated terrain.
use crate::{CurveKind, Edge, EdgeId, FaceId, Mesh, MeshError, VertexId};
use rysmo_math::{DVec3, EPS, closest_on_segment};
/// A validated replacement for the diagonal shared by exactly two triangles.
struct FlipPlan {
    edge: Edge,
    faces: [FaceId; 2],
    triangles: [[VertexId; 3]; 2],
}

impl Mesh {
    /// Inserts a point into a terrain triangle or shared edge, then moves it.
    pub fn add_terrain_detail(
        &mut self,
        face: FaceId,
        point: DVec3,
        offset: DVec3,
    ) -> Result<(VertexId, bool), MeshError> {
        if !point.is_finite() || !offset.is_finite() {
            return Err(MeshError::Invalid("detail point must be finite".into()));
        }
        let original = self.face(face).cloned().ok_or(MeshError::Missing(face.0))?;
        if original.outer.len() != 3 || !original.holes.is_empty() || !self.face_contains(face, point) {
            return Err(MeshError::Invalid("Add Detail requires a point on a terrain triangle".into()));
        }
        if let Some(vertex) = original.outer.iter().find(|id| self.pos(**id).distance(point) < EPS).copied() {
            if self
                .faces_of_vertex(vertex)
                .iter()
                .any(|id| self.face(*id).is_none_or(|f| f.outer.len() != 3 || !f.holes.is_empty()))
            {
                return Err(MeshError::Invalid("detail vertex touches non-triangle geometry".into()));
            }
            if offset.length() < EPS {
                return Ok((vertex, false));
            }
            self.deform_vertices(&std::collections::BTreeSet::from([vertex]), |p| p + offset)?;
            return Ok((vertex, true));
        }
        let edge = self.face_edges(face).into_iter().find(|id| {
            let data = self.edge(*id).unwrap();
            closest_on_segment(self.pos(data.v[0]), self.pos(data.v[1]), point).0.distance(point) < EPS
        });
        let affected = if let Some(edge) = edge { self.faces_of_edge(edge) } else { vec![face] };
        if affected.len() > 2
            || affected.iter().any(|id| self.face(*id).is_none_or(|f| f.outer.len() != 3 || !f.holes.is_empty()))
        {
            return Err(MeshError::Invalid("detail edge touches non-triangle geometry".into()));
        }
        let originals = affected.iter().map(|id| (*id, self.face(*id).unwrap().clone())).collect::<Vec<_>>();
        let mut candidate = self.clone();
        let vertex = VertexId(candidate.reserve_id());
        candidate.vertices.insert(vertex, crate::Vertex { pos: point });
        let split_vertices = edge.map(|id| self.edge(id).unwrap().v);
        if let Some(edge) = edge {
            let curve = candidate.edge(edge).unwrap().curve;
            candidate.split_edge(edge, vertex);
            if let Some(curve) = curve {
                let count = candidate.curve_edges(curve).len();
                if let Some(c) = candidate.curves.get_mut(&curve) {
                    c.kind = CurveKind::Polyline;
                    c.segments = count as u32;
                }
            }
        }
        for (id, old) in originals {
            let triangles = if let Some(endpoints) = split_vertices {
                let index = (0..3)
                    .find(|i| endpoints.contains(&old.outer[*i]) && endpoints.contains(&old.outer[(*i + 1) % 3]))
                    .unwrap();
                let a = old.outer[index];
                let b = old.outer[(index + 1) % 3];
                let c = old.outer[(index + 2) % 3];
                vec![vec![a, vertex, c], vec![vertex, b, c]]
            } else {
                let [a, b, c] = [old.outer[0], old.outer[1], old.outer[2]];
                vec![vec![a, b, vertex], vec![b, c, vertex], vec![c, a, vertex]]
            };
            for (index, outer) in triangles.into_iter().enumerate() {
                for (a, b) in outer.iter().zip(outer.iter().cycle().skip(1)).take(3) {
                    if candidate.edge_between(*a, *b).is_none() {
                        let edge = candidate.raw_edge(*a, *b);
                        let data = candidate.edges.get_mut(&edge).unwrap();
                        data.soft = true;
                        data.smooth = true;
                    }
                }
                let mut new_face = old.clone();
                new_face.outer = outer;
                if index == 0 {
                    candidate.faces.insert(id, new_face);
                } else {
                    candidate.raw_face(new_face);
                }
            }
        }
        if offset.length() > 0. {
            candidate.deform_vertices(&std::collections::BTreeSet::from([vertex]), |p| p + offset)?;
        }
        candidate.validate()?;
        *self = candidate;
        Ok((vertex, true))
    }

    /// Replaces the diagonal shared by two triangles. Face attributes/IDs survive;
    /// the diagonal receives a new ID. Invalid/non-convex configurations are atomic.
    fn terrain_flip_plan(&self, edge: EdgeId) -> Result<FlipPlan, MeshError> {
        let original = self.edge(edge).cloned().ok_or(MeshError::Missing(edge.0))?;
        let faces: [FaceId; 2] = self
            .faces_of_edge(edge)
            .try_into()
            .map_err(|_| MeshError::Invalid("Flip Edge requires two adjacent triangles".into()))?;
        let first = self.face(faces[0]).ok_or(MeshError::Missing(faces[0].0))?;
        let second = self.face(faces[1]).ok_or(MeshError::Missing(faces[1].0))?;
        if [first, second].iter().any(|f| f.outer.len() != 3 || !f.holes.is_empty()) {
            return Err(MeshError::Invalid("Flip Edge requires triangle faces".into()));
        }
        let index = (0..3)
            .find(|i| original.v.contains(&first.outer[*i]) && original.v.contains(&first.outer[(*i + 1) % 3]))
            .ok_or_else(|| MeshError::Invalid("shared edge is missing from the triangle outline".into()))?;
        let a = first.outer[index];
        let b = first.outer[(index + 1) % 3];
        let c = first.outer[(index + 2) % 3];
        let d = *second
            .outer
            .iter()
            .find(|id| !original.v.contains(id))
            .ok_or_else(|| MeshError::Invalid("triangles overlap".into()))?;
        if c == d || self.edge_between(c, d).is_some() {
            return Err(MeshError::Invalid("opposite diagonal already exists".into()));
        }
        let normal = (self.face_normal(faces[0]).unwrap_or_default() + self.face_normal(faces[1]).unwrap_or_default())
            .normalize_or_zero();
        let triangle1 = [c, d, b];
        let triangle2 = [d, c, a];
        for triangle in [&triangle1, &triangle2] {
            let cross =
                (self.pos(triangle[1]) - self.pos(triangle[0])).cross(self.pos(triangle[2]) - self.pos(triangle[0]));
            if cross.length() < EPS * EPS || cross.normalize().dot(normal) <= 1e-6 {
                return Err(MeshError::Invalid("terrain quadrilateral is not convex".into()));
            }
        }
        Ok(FlipPlan { edge: original, faces, triangles: [triangle1, triangle2] })
    }
    #[must_use]
    pub fn can_flip_terrain_edge(&self, edge: EdgeId) -> bool {
        self.terrain_flip_plan(edge).is_ok()
    }
    pub fn flip_terrain_edge(&mut self, edge: EdgeId) -> Result<EdgeId, MeshError> {
        let FlipPlan { edge: original, faces, triangles } = self.terrain_flip_plan(edge)?;
        let [c, d, _] = triangles[0];
        let mut candidate = self.clone();
        candidate.remove_edge_raw(edge);
        let new_edge = candidate.raw_edge(c, d);
        candidate.edges.get_mut(&new_edge).ok_or(MeshError::Missing(new_edge.0))?.set_flags(original.flags());
        for (face, triangle) in faces.into_iter().zip(triangles) {
            candidate.faces.get_mut(&face).ok_or(MeshError::Missing(face.0))?.outer = triangle.to_vec();
        }
        if let Some(curve) = original.curve {
            let count = candidate.curve_edges(curve).len();
            if let Some(descriptor) = candidate.curves.get_mut(&curve) {
                descriptor.kind = CurveKind::Polyline;
                descriptor.segments = count as u32;
            }
            candidate.prune_curves();
        }
        candidate.validate()?;
        *self = candidate;
        Ok(new_edge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Curve, Edge, Face, FaceId, MaterialId, Vertex, VertexId};
    use rysmo_math::DVec3;
    use std::collections::BTreeMap;
    fn sample() -> Mesh {
        let vertices = [DVec3::ZERO, DVec3::X, DVec3::new(1., 1., 0.2), DVec3::Y]
            .into_iter()
            .enumerate()
            .map(|(i, pos)| (VertexId(i as u64 + 1), Vertex { pos }))
            .collect();
        let edges = [[1, 2], [2, 3], [3, 1], [3, 4], [4, 1]]
            .into_iter()
            .enumerate()
            .map(|(i, [a, b])| (EdgeId(i as u64 + 5), Edge::new(VertexId(a), VertexId(b))))
            .collect();
        let faces = [[1, 2, 3], [1, 3, 4]]
            .into_iter()
            .enumerate()
            .map(|(i, outer)| {
                (
                    FaceId(i as u64 + 10),
                    Face {
                        hidden: false,
                        outer: outer.map(VertexId).to_vec(),
                        holes: vec![],
                        material: Some(MaterialId(i as u64 + 42)),
                        back_material: Some(MaterialId(90)),
                        uv: None,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        Mesh::from_parts(vertices, edges, faces).unwrap()
    }
    #[test]
    fn boundary_detail_preserves_neighbour_and_face_materials() {
        let mut mesh = sample();
        let neighbour = mesh.face(FaceId(11)).unwrap().clone();
        let (vertex, changed) = mesh.add_terrain_detail(FaceId(10), DVec3::new(0.5, 0., 0.), DVec3::Z * 0.25).unwrap();
        assert!(changed);
        assert_eq!(mesh.pos(vertex), DVec3::new(0.5, 0., 0.25));
        assert_eq!(mesh.faces().count(), 3);
        assert_eq!(mesh.face(FaceId(11)), Some(&neighbour));
        assert_eq!(mesh.faces().filter(|(_, f)| f.material == Some(MaterialId(42))).count(), 2);
        mesh.validate().unwrap();
    }
    #[test]
    fn concave_quadrilateral_is_rejected_atomically() {
        let mut mesh = sample();
        mesh.vertices.get_mut(&VertexId(1)).unwrap().pos = DVec3::new(0.6, 0.6, 0.);
        mesh.vertices.get_mut(&VertexId(3)).unwrap().pos = DVec3::new(1., 1., 0.);
        mesh.validate().unwrap();
        let before = mesh.clone();
        assert!(mesh.flip_terrain_edge(EdgeId(7)).is_err());
        assert_eq!(mesh, before);
    }
    #[test]
    fn nonplanar_flip_keeps_attributes_and_updates_curve_membership() {
        let mut mesh = sample();
        let curve = mesh
            .register_curve(
                &[EdgeId(5), EdgeId(7)],
                Curve {
                    kind: CurveKind::Polyline,
                    faceted: false,
                    centre: DVec3::ZERO,
                    normal: DVec3::Z,
                    start: DVec3::X,
                    radius: 0.,
                    segments: 2,
                    sweep: 0.,
                },
            )
            .unwrap();
        let old_faces = mesh.faces().map(|(id, f)| (id, f.material, f.back_material, f.uv.clone())).collect::<Vec<_>>();
        let edge = mesh.flip_terrain_edge(EdgeId(7)).unwrap();
        assert!(mesh.edge(edge).unwrap().curve.is_none());
        assert_eq!(mesh.curve(curve).unwrap().segments, 1);
        assert_eq!(
            mesh.faces().map(|(id, f)| (id, f.material, f.back_material, f.uv.clone())).collect::<Vec<_>>(),
            old_faces
        );
        mesh.validate().unwrap();
    }
    #[test]
    fn boundary_or_missing_edges_fail_without_mutation() {
        let mut mesh = sample();
        let before = mesh.clone();
        for edge in [EdgeId(5), EdgeId(u64::MAX)] {
            assert!(mesh.flip_terrain_edge(edge).is_err());
            assert_eq!(mesh, before);
        }
    }
}
