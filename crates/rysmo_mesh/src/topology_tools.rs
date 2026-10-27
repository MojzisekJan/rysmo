//! Curve grouping and consistent face orientation without rebuilding geometry.
use crate::{Curve, CurveId, CurveKind, EdgeId, FaceId, Mesh, MeshError, VertexId};
use rysmo_math::DVec3;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

fn invalid(message: &str) -> MeshError {
    MeshError::Invalid(message.into())
}
fn selected_edges(mesh: &Mesh, edges: &[EdgeId]) -> Result<BTreeSet<EdgeId>, MeshError> {
    if edges.is_empty() || edges.len() > 8192 {
        return Err(invalid("Select between 1 and 8192 editable edges"));
    }
    let selected: BTreeSet<_> = edges.iter().copied().collect();
    for id in &selected {
        let edge = mesh.edge(*id).ok_or(MeshError::Missing(id.0))?;
        if edge.procedural {
            return Err(invalid("Component-opening boundaries are not editable"));
        }
    }
    Ok(selected)
}
impl Mesh {
    /// Groups one open chain or closed loop as a selectable polyline. Returns whether it changed.
    pub fn weld_edges(&mut self, edges: &[EdgeId]) -> Result<bool, MeshError> {
        let selected = selected_edges(self, edges)?;
        if selected.len() < 2 {
            return Err(invalid("Weld needs at least two connected edges"));
        }
        let mut adjacency: BTreeMap<VertexId, Vec<EdgeId>> = BTreeMap::new();
        let curves: BTreeSet<_> = selected.iter().filter_map(|id| self.edges[id].curve).collect();
        for (id, edge) in self.edges() {
            if edge.curve.is_some_and(|curve| curves.contains(&curve)) && !selected.contains(&id) {
                return Err(invalid("Select every edge of an existing curve before welding it"));
            }
        }
        for id in &selected {
            for vertex in self.edges[id].v {
                adjacency.entry(vertex).or_default().push(*id);
            }
        }
        if adjacency.values().any(|edges| edges.len() > 2) {
            return Err(MeshError::NotAChain);
        }
        let seed = *selected.first().expect("at least two edges");
        let mut visited = BTreeSet::from([seed]);
        let mut queue = VecDeque::from([seed]);
        while let Some(edge) = queue.pop_front() {
            for vertex in self.edges[&edge].v {
                for next in &adjacency[&vertex] {
                    if visited.insert(*next) {
                        queue.push_back(*next);
                    }
                }
            }
        }
        if visited != selected {
            return Err(MeshError::NotAChain);
        }
        if curves.len() == 1 && selected.iter().all(|id| self.edges[id].curve.is_some()) {
            return Ok(false);
        }
        let members: Vec<_> = selected.into_iter().collect();
        let id = CurveId(self.reserve_id());
        self.curves.insert(
            id,
            Curve {
                kind: CurveKind::Polyline,
                faceted: false,
                centre: DVec3::ZERO,
                normal: DVec3::Z,
                start: DVec3::X,
                radius: 1.,
                segments: members.len() as u32,
                sweep: 0.,
            },
        );
        for edge in members {
            self.edges.get_mut(&edge).expect("validated member").curve = Some(id);
        }
        for old in curves {
            self.curves.remove(&old);
        }
        Ok(true)
    }

    /// Ungroups complete curves touched by the selection, keeping their individual edges.
    pub fn explode_curves(&mut self, edges: &[EdgeId]) -> Result<Vec<EdgeId>, MeshError> {
        let selected = selected_edges(self, edges)?;
        let curves: BTreeSet<_> = selected.iter().filter_map(|id| self.edges[id].curve).collect();
        if curves.is_empty() {
            return Err(invalid("Select at least one curve to explode"));
        }
        let mut members = Vec::new();
        for (id, edge) in self.edges() {
            if edge.curve.is_some_and(|curve| curves.contains(&curve)) {
                if members.len() == 8192 || edge.procedural {
                    return Err(invalid("Curve explosion exceeds 8192 edges or touches a component-opening boundary"));
                }
                members.push(id);
            }
        }
        for id in &members {
            self.edges.get_mut(id).expect("collected member").curve = None;
        }
        for curve in curves {
            self.curves.remove(&curve);
        }
        Ok(members)
    }

    /// Computes necessary reversals through visible manifold neighbours, preserving the seed.
    pub fn orientation_plan(&self, seed: FaceId) -> Result<Vec<FaceId>, MeshError> {
        let face = self.face(seed).ok_or(MeshError::Missing(seed.0))?;
        if face.hidden {
            return Err(invalid("Unhide the reference face before orienting it"));
        }
        if self.face_count() > 100_000 {
            return Err(invalid("Face orientation exceeds 100000 faces"));
        }
        let mut adjacency: BTreeMap<(VertexId, VertexId), Vec<(FaceId, bool)>> = BTreeMap::new();
        let mut steps: BTreeMap<FaceId, Vec<(VertexId, VertexId)>> = BTreeMap::new();
        let mut count = 0usize;
        for (id, face) in self.faces() {
            let mut boundary = Vec::new();
            for outline in face.loops() {
                count = count.saturating_add(outline.len());
                if count > 500_000 {
                    return Err(invalid("Face orientation exceeds 500000 boundary steps"));
                }
                for i in 0..outline.len() {
                    let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
                    let key = if a < b { (a, b) } else { (b, a) };
                    adjacency.entry(key).or_default().push((id, a < b));
                    boundary.push((a, b));
                }
            }
            steps.insert(id, boundary);
        }
        let mut flip = BTreeMap::from([(seed, false)]);
        let mut queue = VecDeque::from([seed]);
        while let Some(face) = queue.pop_front() {
            for &(a, b) in &steps[&face] {
                let edge = self.edge_between(a, b).ok_or_else(|| invalid("A face boundary edge is missing"))?;
                if self.edges[&edge].procedural {
                    return Err(invalid("Face orientation touches a component-opening boundary"));
                }
                let key = if a < b { (a, b) } else { (b, a) };
                let neighbours = &adjacency[&key];
                if neighbours.len() > 2 {
                    return Err(invalid("Face orientation is ambiguous at a non-manifold edge"));
                }
                for &(next, direction) in neighbours {
                    if next == face || self.faces[&next].hidden {
                        continue;
                    }
                    let required = flip[&face] ^ ((a < b) == direction);
                    if let Some(previous) = flip.get(&next) {
                        if *previous != required {
                            return Err(invalid("Connected faces have conflicting orientation constraints"));
                        }
                    } else {
                        flip.insert(next, required);
                        queue.push_back(next);
                    }
                }
            }
        }
        Ok(flip.into_iter().filter_map(|(id, reverse)| reverse.then_some(id)).collect())
    }
    pub fn orient_faces(&mut self, seed: FaceId) -> Result<Vec<FaceId>, MeshError> {
        let flipped = self.orientation_plan(seed)?;
        self.reverse_faces(&flipped);
        Ok(flipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Face, MaterialId, TexturePlacement};
    fn triangle(mesh: &mut Mesh, points: [DVec3; 3]) -> FaceId {
        let outer: Vec<_> = points.into_iter().map(|p| mesh.add_vertex(p)).collect();
        for i in 0..3 {
            mesh.raw_edge(outer[i], outer[(i + 1) % 3]);
        }
        mesh.raw_face(Face { outer, holes: vec![], hidden: false, material: None, back_material: None, uv: None })
    }
    #[test]
    fn welding_and_exploding_only_change_grouping_and_keep_edge_styles() {
        for closed in [false, true] {
            let mut mesh = Mesh::default();
            let mut lines =
                vec![(DVec3::ZERO, DVec3::X), (DVec3::X, DVec3::X + DVec3::Y), (DVec3::X + DVec3::Y, DVec3::Y)];
            if closed {
                lines.push((DVec3::Y, DVec3::ZERO));
            }
            let edges = mesh.add_lines(&lines, Some(DVec3::Z)).unwrap();
            mesh.set_edge_flags(&edges, Some(true), Some(true), Some(true));
            for id in &edges {
                mesh.paint_edge(*id, Some(MaterialId(999))).unwrap();
            }
            let before = mesh.clone();
            assert!(mesh.weld_edges(&edges).unwrap());
            assert_eq!(mesh.curves().count(), 1);
            let (curve, descriptor) = mesh.curves().next().unwrap();
            assert_eq!(descriptor.kind, CurveKind::Polyline);
            assert_eq!(mesh.curve_edges(curve).len(), edges.len());
            assert!(!mesh.weld_edges(&edges).unwrap());
            assert_eq!(mesh.explode_curves(&[edges[0]]).unwrap().len(), edges.len());
            assert!(mesh.curves().next().is_none());
            assert_eq!(mesh.vertices, before.vertices);
            assert_eq!(mesh.edges, before.edges);
            assert_eq!(mesh.faces, before.faces);
            mesh.validate().unwrap();
        }
    }
    #[test]
    fn branches_disconnected_and_partial_curves_are_rejected_atomically() {
        let mut mesh = Mesh::default();
        let edges = mesh
            .add_lines(
                &[
                    (DVec3::ZERO, DVec3::X),
                    (DVec3::X, DVec3::X * 2.),
                    (DVec3::X, DVec3::X + DVec3::Y),
                    (DVec3::Y * 3., DVec3::Y * 3. + DVec3::X),
                ],
                None,
            )
            .unwrap();
        let before = mesh.clone();
        assert!(mesh.weld_edges(&edges[..3]).is_err());
        assert!(mesh.weld_edges(&[edges[0], edges[3]]).is_err());
        assert!(mesh.weld_edges(&[edges[0], EdgeId(u64::MAX)]).is_err());
        assert!(mesh.explode_curves(&[edges[0]]).is_err());
        assert_eq!(mesh, before);
        mesh.weld_edges(&edges[..2]).unwrap();
        let before = mesh.clone();
        assert!(mesh.weld_edges(&[edges[0], edges[2]]).is_err());
        assert_eq!(mesh, before);
        mesh.edges.get_mut(&edges[1]).unwrap().procedural = true;
        let before = mesh.clone();
        assert!(mesh.explode_curves(&[edges[0]]).is_err());
        assert_eq!(mesh, before);
    }
    #[test]
    fn replacing_multiple_complete_curves_removes_only_their_old_descriptors() {
        let mut mesh = Mesh::default();
        let edges = mesh
            .add_lines(&(0..4).map(|i| (DVec3::X * i as f64, DVec3::X * (i + 1) as f64)).collect::<Vec<_>>(), None)
            .unwrap();
        mesh.weld_edges(&edges[..2]).unwrap();
        mesh.weld_edges(&edges[2..]).unwrap();
        assert_eq!(mesh.curves().count(), 2);
        mesh.weld_edges(&edges).unwrap();
        assert_eq!(mesh.curves().count(), 1);
        assert_eq!(mesh.curves().next().unwrap().1.segments, 4);
        mesh.validate().unwrap();
    }
    #[test]
    fn orientation_preserves_seed_ids_uv_and_physical_material_sides() {
        let mut mesh = Mesh::default();
        let seed = triangle(&mut mesh, [DVec3::ZERO, DVec3::X, DVec3::Y]);
        let neighbour = triangle(&mut mesh, [DVec3::X, DVec3::X + DVec3::Y, DVec3::Y]);
        mesh.reverse_faces(&[neighbour]);
        let face = mesh.faces.get_mut(&neighbour).unwrap();
        face.material = Some(MaterialId(800));
        face.back_material = Some(MaterialId(801));
        face.uv = Some(TexturePlacement { rotation: 35., offset: [0.2, 0.4], ..Default::default() });
        let before = mesh.clone();
        assert_eq!(mesh.orient_faces(seed).unwrap(), vec![neighbour]);
        assert_eq!(mesh.face(seed), before.face(seed));
        assert_eq!(mesh.face(neighbour).unwrap().material, Some(MaterialId(801)));
        assert_eq!(mesh.face(neighbour).unwrap().back_material, Some(MaterialId(800)));
        assert_eq!(mesh.face(neighbour).unwrap().uv, before.face(neighbour).unwrap().uv);
        assert!(mesh.face_normal(seed).unwrap().dot(mesh.face_normal(neighbour).unwrap()) > 0.999);
        assert!(mesh.orient_faces(seed).unwrap().is_empty());
        mesh.validate().unwrap();
    }
    #[test]
    fn hidden_faces_are_barriers_and_non_manifold_neighbours_fail_atomically() {
        let mut mesh = Mesh::default();
        let seed = triangle(&mut mesh, [DVec3::ZERO, DVec3::X, DVec3::Y]);
        let hidden = triangle(&mut mesh, [DVec3::X, DVec3::X + DVec3::Y, DVec3::Y]);
        mesh.reverse_faces(&[hidden]);
        mesh.faces.get_mut(&hidden).unwrap().hidden = true;
        let before = mesh.clone();
        assert!(mesh.orient_faces(seed).unwrap().is_empty());
        assert_eq!(mesh, before);
        assert!(mesh.orient_faces(hidden).is_err());
        triangle(&mut mesh, [DVec3::X, DVec3::Y, DVec3::Z]);
        let before = mesh.clone();
        assert!(mesh.orient_faces(seed).is_err());
        assert_eq!(mesh, before);
    }
    #[test]
    fn non_orientable_strip_is_rejected_without_partial_reversals() {
        let mut mesh = Mesh::default();
        let a = [DVec3::new(2., 0., 0.), DVec3::new(-0.75, 1.3, 0.43), DVec3::new(-0.75, -1.3, 0.43)];
        let b = [DVec3::new(1., 0., 0.), DVec3::new(-0.75, 1.3, -0.43), DVec3::new(-0.75, -1.3, -0.43)];
        let mut seed = None;
        for [p, q, r, s] in [[a[0], a[1], b[1], b[0]], [a[1], a[2], b[2], b[1]], [a[2], b[0], a[0], b[2]]] {
            let id = triangle(&mut mesh, [p, q, r]);
            seed.get_or_insert(id);
            triangle(&mut mesh, [p, r, s]);
        }
        mesh.validate().unwrap();
        let before = mesh.clone();
        let error = mesh.orient_faces(seed.unwrap()).unwrap_err();
        assert!(error.to_string().contains("conflicting"), "{error}");
        assert_eq!(mesh, before);
    }
}
