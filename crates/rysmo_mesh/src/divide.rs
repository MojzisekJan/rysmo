//! Equal subdivisions without rebuilding adjacent faces or their appearances.
use crate::{EdgeId, Mesh, MeshError};
use rysmo_math::{DVec3, EPS};
use std::collections::BTreeSet;

pub struct EdgeDivision {
    pub edge: EdgeId,
    /// Interior subdivision points, in edge order.
    pub points: Vec<DVec3>,
}
impl Mesh {
    pub fn divide_edges_plan(&self, edges: &[EdgeId], segments: u32) -> Result<Vec<EdgeDivision>, MeshError> {
        let invalid = |text: &str| MeshError::Invalid(text.into());
        if !(2..=512).contains(&segments) {
            return Err(invalid("Choose between 2 and 512 segments per edge"));
        }
        let edges: BTreeSet<_> = edges.iter().copied().collect();
        if edges.is_empty() || edges.len() > 8192 / segments as usize {
            return Err(invalid("Select edges producing at most 8192 segments"));
        }
        edges
            .into_iter()
            .map(|id| {
                let edge = self.edge(id).ok_or(MeshError::Missing(id.0))?;
                if edge.procedural || edge.curve.is_some() {
                    return Err(invalid(
                        "Select straight editable edges; use curve segment settings for arcs and circles",
                    ));
                }
                let (a, b) = self.edge_points(id);
                let length = a.distance(b);
                if !length.is_finite() || length / segments as f64 <= EPS * 2. {
                    return Err(invalid("Subdivisions would be too small for the model tolerance"));
                }
                Ok(EdgeDivision {
                    edge: id,
                    points: (1..segments).map(|i| a.lerp(b, i as f64 / segments as f64)).collect(),
                })
            })
            .collect()
    }

    /// Splits all selected straight edges in one atomic edit, preserving adjacent faces.
    pub fn divide_edges(&mut self, edges: &[EdgeId], segments: u32) -> Result<Vec<EdgeId>, MeshError> {
        let plan = self.divide_edges_plan(edges, segments)?;
        let mut staged = self.clone();
        let mut created = Vec::new();
        for division in plan {
            let mut remaining = division.edge;
            for point in division.points {
                let vertex = staged.add_vertex(point);
                let (first, second) = staged.split_edge(remaining, vertex);
                created.push(first);
                remaining = second;
            }
            created.push(remaining);
        }
        staged.validate()?;
        *self = staged;
        Ok(created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MaterialId,
        tests::{only_face, rect},
    };

    #[test]
    fn face_boundary_division_preserves_identity_appearance_and_edge_styles() {
        let mut mesh = Mesh::default();
        let edges = rect(&mut mesh, 0., 0., 8., 4., 0.);
        let face = only_face(&mesh);
        mesh.faces.get_mut(&face).unwrap().material = Some(MaterialId(900));
        let uv = crate::TexturePlacement { rotation: 0.3, ..Default::default() };
        mesh.faces.get_mut(&face).unwrap().uv = Some(uv);
        mesh.set_edge_flags(&[edges[0]], Some(true), Some(true), Some(true));
        mesh.paint_edge(edges[0], Some(MaterialId(901))).unwrap();
        let (a, b) = mesh.edge_points(edges[0]);
        let out = mesh.divide_edges(&[edges[0], edges[0]], 4).unwrap();
        assert_eq!(out.len(), 4);
        assert_eq!(mesh.edge_count(), 7);
        assert_eq!(mesh.face_count(), 1);
        let f = mesh.face(face).unwrap();
        assert_eq!(f.outer.len(), 7);
        assert_eq!(f.material, Some(MaterialId(900)));
        assert_eq!(f.uv, Some(uv));
        for id in out {
            let edge = mesh.edge(id).unwrap();
            assert!(edge.soft && edge.smooth && edge.hidden);
            assert_eq!(edge.material, Some(MaterialId(901)));
            let (p, q) = mesh.edge_points(id);
            assert!((p.distance(q) - a.distance(b) / 4.).abs() < EPS);
        }
        mesh.validate().unwrap();
    }

    #[test]
    fn invalid_batch_is_atomic_and_work_is_bounded() {
        let mut mesh = Mesh::default();
        let edges = rect(&mut mesh, 0., 0., 8., 4., 0.);
        let before = mesh.clone();
        for (selected, segments) in
            [(vec![edges[0]], 1), (vec![edges[0]], 513), (vec![edges[0], EdgeId(u64::MAX)], 3), (vec![], 2)]
        {
            assert!(mesh.divide_edges(&selected, segments).is_err());
            assert_eq!(mesh, before);
        }
        assert!(mesh.divide_edges_plan(&(0..17).map(EdgeId).collect::<Vec<_>>(), 512).is_err());
        mesh.edges.get_mut(&edges[0]).unwrap().procedural = true;
        let before = mesh.clone();
        assert!(mesh.divide_edges(&[edges[0]], 3).is_err());
        assert_eq!(mesh, before);
    }

    #[test]
    fn tolerance_failure_does_not_make_collapsed_segments() {
        let mut mesh = Mesh::default();
        let edge = mesh.add_lines(&[(DVec3::ZERO, DVec3::X * EPS * 10.)], None).unwrap()[0];
        let before = mesh.clone();
        assert!(mesh.divide_edges(&[edge], 20).is_err());
        assert_eq!(mesh, before);
    }
}
