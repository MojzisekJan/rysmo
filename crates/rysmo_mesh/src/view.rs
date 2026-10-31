//! Lookups in a mesh that outlast the edit they were built for.
//!
//! Edits reach the element maps in many places, so nothing here is told about them.
//! Trees are built for a snapshot of the maps, and `Mesh::view` compares the mesh with
//! that snapshot and adds small trees for the difference. The comparison skips the blocks
//! the two share, so its cost follows the size of the change, not of the mesh.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use rysmo_math::{EPS, Plane};

use crate::blocks::BlockMap;
use crate::ids::{CurveId, EdgeId, FaceId, VertexId};
use crate::mesh::{Edge, Face, Mesh, Vertex};
use crate::spatial::{Aabb, Bvh};

/// How far the tolerances reach past an element's box.
const MARGIN: f64 = EPS * 4.0;
/// The snapshot is replaced once changes reach one part in this many.
const REBUILD_SHARE: usize = 8;

/// The trees of a snapshot of a mesh.
#[derive(Debug)]
struct Snapshot {
    vertices: BlockMap<VertexId, Vertex>,
    edges: BlockMap<EdgeId, Edge>,
    faces: BlockMap<FaceId, Face>,
    vertex_tree: Bvh<VertexId>,
    edge_tree: Bvh<EdgeId>,
    face_tree: Bvh<FaceId>,
    around: HashMap<VertexId, Vec<FaceId>>,
    curves: HashMap<CurveId, Vec<EdgeId>>,
}

/// The latest snapshot, shared by a mesh and its clones. It is only ever an earlier
/// state to compare with, so it plays no part in what a mesh is.
#[derive(Clone, Debug, Default)]
pub(crate) struct SnapshotSlot(Arc<Mutex<Option<Arc<Snapshot>>>>);

impl PartialEq for SnapshotSlot {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

fn vertex_box(mesh: &Mesh, id: VertexId) -> Option<(Aabb, VertexId)> {
    Some((Aabb::at(mesh.vertex(id)?.pos).expanded(MARGIN), id))
}

fn edge_box(mesh: &Mesh, id: EdgeId) -> Option<(Aabb, EdgeId)> {
    let edge = mesh.edge(id)?;
    Some((Aabb::of(edge.v.map(|v| mesh.pos(v))).expanded(MARGIN), id))
}

fn face_box(mesh: &Mesh, id: FaceId) -> Option<(Aabb, FaceId)> {
    let face = mesh.face(id)?;
    Some((Aabb::of(face.outer.iter().map(|v| mesh.pos(*v))).expanded(MARGIN), id))
}

fn faces_around<'a>(faces: impl Iterator<Item = (FaceId, &'a Face)>) -> HashMap<VertexId, Vec<FaceId>> {
    let mut around: HashMap<VertexId, Vec<FaceId>> = HashMap::new();
    for (id, face) in faces {
        for v in face.loops().flatten() {
            around.entry(*v).or_default().push(id);
        }
    }
    around
}

fn curve_lists<'a>(edges: impl Iterator<Item = (EdgeId, &'a Edge)>) -> HashMap<CurveId, Vec<EdgeId>> {
    let mut curves: HashMap<CurveId, Vec<EdgeId>> = HashMap::new();
    for (id, edge) in edges {
        if let Some(curve) = edge.curve {
            curves.entry(curve).or_default().push(id);
        }
    }
    curves
}

impl Snapshot {
    fn of(mesh: &Mesh) -> Self {
        Snapshot {
            vertices: mesh.vertices.clone(),
            edges: mesh.edges.clone(),
            faces: mesh.faces.clone(),
            vertex_tree: Bvh::new(mesh.vertices().filter_map(|(id, _)| vertex_box(mesh, id)).collect()),
            edge_tree: Bvh::new(mesh.edges().filter_map(|(id, _)| edge_box(mesh, id)).collect()),
            face_tree: Bvh::new(mesh.faces().filter_map(|(id, _)| face_box(mesh, id)).collect()),
            around: faces_around(mesh.faces()),
            curves: curve_lists(mesh.edges()),
        }
    }

    fn size(&self) -> usize {
        self.vertices.len() + self.edges.len() + self.faces.len()
    }
}

/// Lookups in one state of a mesh. Pass the same mesh to its methods; while that mesh
/// changes, new elements are missed but nothing wrong is reported about the rest,
/// since results are checked against the mesh. Take a new view to see new elements.
#[derive(Debug)]
pub struct MeshView {
    base: Arc<Snapshot>,
    vertices: Bvh<VertexId>,
    edges: Bvh<EdgeId>,
    faces: Bvh<FaceId>,
    around: HashMap<VertexId, Vec<FaceId>>,
    curves: HashMap<CurveId, Vec<EdgeId>>,
}

fn sorted<T: Ord>(mut found: Vec<T>) -> Vec<T> {
    found.sort_unstable();
    found.dedup();
    found
}

impl MeshView {
    /// The vertices within `area`, in id order.
    pub fn vertices_in(&self, mesh: &Mesh, area: Aabb) -> Vec<VertexId> {
        let mut found = Vec::new();
        self.base.vertex_tree.overlapping(area, |id| found.push(*id));
        self.vertices.overlapping(area, |id| found.push(*id));
        found.retain(|id| vertex_box(mesh, *id).is_some_and(|(b, _)| b.overlaps(&area)));
        sorted(found)
    }

    /// The edges whose boxes reach `area`, in id order.
    pub fn edges_in(&self, mesh: &Mesh, area: Aabb) -> Vec<EdgeId> {
        self.edges_where(mesh, |b| b.overlaps(&area))
    }

    /// The faces whose outlines' boxes reach `area`, in id order.
    pub fn faces_in(&self, mesh: &Mesh, area: Aabb) -> Vec<FaceId> {
        self.faces_where(mesh, |b| b.overlaps(&area))
    }

    pub fn edges_near_plane(&self, mesh: &Mesh, plane: &Plane) -> Vec<EdgeId> {
        self.edges_where(mesh, |b| b.near_plane(plane, MARGIN))
    }

    pub fn faces_near_plane(&self, mesh: &Mesh, plane: &Plane) -> Vec<FaceId> {
        self.faces_where(mesh, |b| b.near_plane(plane, MARGIN))
    }

    /// The edges whose boxes `enter` accepts, in id order. `enter` also decides which
    /// branches of the trees to go down, so it must accept any box around one it accepts.
    pub fn edges_where(&self, mesh: &Mesh, mut enter: impl FnMut(&Aabb) -> bool) -> Vec<EdgeId> {
        let mut found = Vec::new();
        self.base.edge_tree.visit(&mut enter, |id| found.push(*id));
        self.edges.visit(&mut enter, |id| found.push(*id));
        found.retain(|id| edge_box(mesh, *id).is_some_and(|(b, _)| enter(&b)));
        sorted(found)
    }

    /// See `edges_where`.
    pub fn faces_where(&self, mesh: &Mesh, mut enter: impl FnMut(&Aabb) -> bool) -> Vec<FaceId> {
        let mut found = Vec::new();
        self.base.face_tree.visit(&mut enter, |id| found.push(*id));
        self.faces.visit(&mut enter, |id| found.push(*id));
        found.retain(|id| face_box(mesh, *id).is_some_and(|(b, _)| enter(&b)));
        sorted(found)
    }

    /// The faces with a loop through `v`, in id order.
    pub fn faces_at(&self, mesh: &Mesh, v: VertexId) -> Vec<FaceId> {
        let known = self.base.around.get(&v).into_iter().flatten();
        let mut found: Vec<_> = known.chain(self.around.get(&v).into_iter().flatten()).copied().collect();
        found.retain(|id| mesh.face(*id).is_some_and(|f| f.has_vertex(v)));
        sorted(found)
    }

    /// The edges of a curve, in id order.
    pub fn curve_edges(&self, mesh: &Mesh, curve: CurveId) -> Vec<EdgeId> {
        let known = self.base.curves.get(&curve).into_iter().flatten();
        let mut found: Vec<_> = known.chain(self.curves.get(&curve).into_iter().flatten()).copied().collect();
        found.retain(|id| mesh.edge(*id).is_some_and(|e| e.curve == Some(curve)));
        sorted(found)
    }

    /// The faces whose loops run along `e`, in id order.
    pub fn faces_of_edge(&self, mesh: &Mesh, e: EdgeId) -> Vec<FaceId> {
        let Some(edge) = mesh.edge(e) else { return Vec::new() };
        let k = crate::mesh::key(edge.v[0], edge.v[1]);
        let mut found = self.faces_at(mesh, edge.v[0]);
        found.retain(|f| {
            mesh.face(*f).is_some_and(|f| f.loop_edges().iter().any(|(a, b)| crate::mesh::key(*a, *b) == k))
        });
        found
    }

    /// The faces with a loop through any of `vertices`, in id order.
    pub fn faces_touching(&self, mesh: &Mesh, vertices: &BTreeSet<VertexId>) -> BTreeSet<FaceId> {
        vertices.iter().flat_map(|v| self.faces_at(mesh, *v)).collect()
    }
}

impl Mesh {
    /// Lookups in this mesh as it is now.
    pub fn view(&self) -> MeshView {
        let mut slot = self.snapshot.0.lock().expect("no panic while the snapshot is replaced");
        let base = slot.get_or_insert_with(|| Arc::new(Snapshot::of(self))).clone();
        let vertices: Vec<_> = self.vertices.changed_since(&base.vertices);
        let edges: Vec<_> = self.edges.changed_since(&base.edges);
        let faces: Vec<_> = self.faces.changed_since(&base.faces);
        if (vertices.len() + edges.len() + faces.len()) * REBUILD_SHARE > base.size() {
            let base = Arc::new(Snapshot::of(self));
            *slot = Some(base.clone());
            return MeshView {
                base,
                vertices: Bvh::default(),
                edges: Bvh::default(),
                faces: Bvh::default(),
                around: HashMap::new(),
                curves: HashMap::new(),
            };
        }
        drop(slot);
        // What hangs off a moved vertex has moved too.
        let moved = vertices.iter().filter(|v| self.vertices.contains_key(v));
        let faces_moved = moved.clone().flat_map(|v| base.around.get(v).into_iter().flatten()).copied();
        let faces = sorted(faces.iter().copied().chain(faces_moved).collect());
        let edges_moved = moved.flat_map(|v| self.edges_of_vertex(*v));
        let curves = curve_lists(edges.iter().filter_map(|id| Some((*id, self.edge(*id)?))));
        let edges = sorted(edges.iter().copied().chain(edges_moved).collect());
        MeshView {
            curves,
            around: faces_around(faces.iter().filter_map(|id| Some((*id, self.face(*id)?)))),
            vertices: Bvh::new(vertices.iter().filter_map(|id| vertex_box(self, *id)).collect()),
            edges: Bvh::new(edges.iter().filter_map(|id| edge_box(self, *id)).collect()),
            faces: Bvh::new(faces.iter().filter_map(|id| face_box(self, *id)).collect()),
            base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Element;
    use crate::tests::{rect, unit_box};
    use rysmo_math::DVec3;

    /// A view must agree with a scan of the mesh, whatever happened since its snapshot.
    fn assert_agrees(mesh: &Mesh) {
        let view = mesh.view();
        let area = |p: DVec3| Aabb::at(p).expanded(0.4);
        for (v, vertex) in mesh.vertices() {
            let scan: Vec<_> = mesh.faces().filter(|(_, f)| f.has_vertex(v)).map(|(id, _)| id).collect();
            assert_eq!(view.faces_at(mesh, v), scan);
            for (curve, _) in mesh.curves() {
                let scan: Vec<_> = mesh.edges().filter(|(_, e)| e.curve == Some(curve)).map(|(id, _)| id).collect();
                assert_eq!(view.curve_edges(mesh, curve), scan);
            }
            let box_at = area(vertex.pos);
            let scan: Vec<_> = mesh
                .edges()
                .filter(|(id, _)| edge_box(mesh, *id).unwrap().0.overlaps(&box_at))
                .map(|(id, _)| id)
                .collect();
            assert_eq!(view.edges_in(mesh, box_at), scan);
            let scan: Vec<_> = mesh
                .faces()
                .filter(|(id, _)| face_box(mesh, *id).unwrap().0.overlaps(&box_at))
                .map(|(id, _)| id)
                .collect();
            assert_eq!(view.faces_in(mesh, box_at), scan);
            let scan: Vec<_> = mesh
                .vertices()
                .filter(|(id, _)| vertex_box(mesh, *id).unwrap().0.overlaps(&box_at))
                .map(|(id, _)| id)
                .collect();
            assert_eq!(view.vertices_in(mesh, box_at), scan);
        }
    }

    #[test]
    fn a_view_follows_every_kind_of_edit() {
        let mut m = Mesh::new();
        for k in 0..80 {
            rect(&mut m, k as f64 * 2., 0., k as f64 * 2. + 1., 1., 0.);
        }
        m.view();
        let first = m.faces().next().unwrap().0;
        m.push_pull(first, 0.5, false).unwrap();
        assert_agrees(&m);
        let edge = m.edges().next().unwrap().0;
        m.move_elements(&[Element::Edge(edge)], DVec3::Z * 0.2).unwrap();
        assert_agrees(&m);
        m.add_line(DVec3::new(2., 0.5, 0.), DVec3::new(3., 0.5, 0.), None).unwrap();
        assert_agrees(&m);
        let face = m.faces().last().unwrap().0;
        m.erase(&[Element::Face(face)]);
        assert_agrees(&m);
        let lines: Vec<_> = (0..12)
            .map(|k| {
                let at = |k: i32| DVec3::new(k as f64, 5. + (k * k) as f64 * 0.1, 0.);
                (at(k), at(k + 1))
            })
            .collect();
        let edges = m.add_lines(&lines, None).unwrap();
        let curve = crate::Curve {
            kind: crate::CurveKind::Polyline,
            faceted: false,
            centre: DVec3::ZERO,
            normal: DVec3::Z,
            start: DVec3::X,
            radius: 1.,
            sweep: 1.,
            segments: 12,
        };
        m.register_curve(&edges, curve).unwrap();
        assert_agrees(&m);
        m.move_elements(&[Element::Edge(edges[3])], DVec3::Z).unwrap();
        assert_agrees(&m);
    }

    #[test]
    fn clones_share_a_snapshot_and_stay_independent() {
        let mut a = unit_box();
        a.view();
        let mut b = a.clone();
        b.add_line(DVec3::new(0., 0., 3.), DVec3::new(1., 0., 3.), None).unwrap();
        assert_agrees(&a);
        assert_agrees(&b);
        a.map_all(|p| p + DVec3::X);
        assert_agrees(&a);
        assert_agrees(&b);
        assert_eq!(a.clone(), a);
    }
}
