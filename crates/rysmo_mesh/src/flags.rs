//! Edge flags (soft, smooth, hidden) and face orientation helpers.

use std::collections::{BTreeSet, HashMap, VecDeque};

use rysmo_math::DVec3;

use crate::ids::{EdgeId, FaceId, VertexId};
use crate::mesh::{Mesh, key};

impl Mesh {
    pub fn paint_edge(&mut self, id: EdgeId, material: Option<crate::MaterialId>) -> Result<(), crate::MeshError> {
        let edge = self.edges.get_mut(&id).ok_or(crate::MeshError::Missing(id.0))?;
        edge.material = material;
        Ok(())
    }

    /// Sets edge flags. `None` leaves a flag as it is.
    pub fn set_edge_flags(&mut self, edges: &[EdgeId], soft: Option<bool>, smooth: Option<bool>, hidden: Option<bool>) {
        for e in edges {
            if let Some(edge) = self.edges.get_mut(e) {
                if let Some(s) = soft {
                    edge.soft = s;
                }
                if let Some(s) = smooth {
                    edge.smooth = s;
                }
                if let Some(h) = hidden {
                    edge.hidden = h;
                }
            }
        }
    }

    /// The angle in degrees between the normals of the two faces along an edge, or
    /// `None` unless exactly two faces meet there.
    pub fn edge_angle(&self, e: EdgeId) -> Option<f64> {
        let faces = self.faces_of_edge(e);
        if faces.len() != 2 {
            return None;
        }
        let a = self.face_normal(faces[0])?;
        let b = self.face_normal(faces[1])?;
        Some(a.dot(b).clamp(-1.0, 1.0).acos().to_degrees())
    }

    /// Softens and smooths the edges where the two faces meet at less than
    /// `max_degrees`, as Soften Edges does. Returns how many changed.
    pub fn soften_by_angle(&mut self, edges: &[EdgeId], max_degrees: f64) -> usize {
        let mut n = 0;
        for e in edges {
            if self.edge_angle(*e).is_some_and(|a| a > 1e-6 && a < max_degrees) {
                let edge = self.edges.get_mut(e).expect("edge with faces exists");
                if !edge.soft || !edge.smooth {
                    edge.soft = true;
                    edge.smooth = true;
                    n += 1;
                }
            }
        }
        n
    }

    /// Faces reachable from `seed` through edges shared by exactly two faces.
    pub fn connected_faces(&self, seed: FaceId) -> BTreeSet<FaceId> {
        let mut seen = BTreeSet::from([seed]);
        let mut queue = VecDeque::from([seed]);
        while let Some(f) = queue.pop_front() {
            for e in self.face_edges(f) {
                let fs = self.faces_of_edge(e);
                if fs.len() == 2 {
                    for g in fs {
                        if seen.insert(g) {
                            queue.push_back(g);
                        }
                    }
                }
            }
        }
        seen
    }

    /// Turns faces round so that each pair of neighbours in `faces` runs its shared
    /// edge in opposite directions, starting from `seed`, which keeps its facing.
    pub fn orient_from(&mut self, seed: FaceId, faces: &BTreeSet<FaceId>) {
        let mut done = BTreeSet::from([seed]);
        let mut queue = VecDeque::from([seed]);
        while let Some(f) = queue.pop_front() {
            let steps = self.faces[&f].loop_edges();
            for (a, b) in steps {
                let Some(e) = self.edge_between(a, b) else { continue };
                let fs = self.faces_of_edge(e);
                if fs.len() != 2 {
                    continue;
                }
                for g in fs {
                    if g == f || done.contains(&g) || !faces.contains(&g) {
                        continue;
                    }
                    let same_way = self.faces[&g].loop_edges().contains(&(a, b));
                    if same_way {
                        self.faces.get_mut(&g).expect("listed").reverse();
                    }
                    done.insert(g);
                    queue.push_back(g);
                }
            }
        }
    }

    /// The signed volume enclosed by faces: positive when they face outwards.
    pub fn signed_volume(&self, faces: &BTreeSet<FaceId>) -> f64 {
        let mut v = 0.0;
        for f in faces {
            for t in crate::triangulate_face(self, *f) {
                let (a, b, c) = (self.pos(t[0]), self.pos(t[1]), self.pos(t[2]));
                v += a.dot(b.cross(c)) / 6.0;
            }
        }
        v
    }

    /// Shading normals for every (face, vertex) corner: the average of the normals of
    /// the faces around the vertex that are joined to the face across smooth edges at
    /// that vertex. Corners with no smooth edge get the face normal. Built in one pass
    /// for the viewport.
    pub fn smooth_normals(&self) -> HashMap<(FaceId, VertexId), DVec3> {
        let mut edge_faces: HashMap<(VertexId, VertexId), Vec<FaceId>> = HashMap::new();
        let mut vertex_faces: HashMap<VertexId, Vec<FaceId>> = HashMap::new();
        for (id, f) in &self.faces {
            for (a, b) in f.loop_edges() {
                edge_faces.entry(key(a, b)).or_default().push(*id);
                vertex_faces.entry(a).or_default().push(*id);
            }
        }
        let mut smooth_at: HashMap<VertexId, Vec<(VertexId, VertexId)>> = HashMap::new();
        for e in self.edges.values().filter(|e| e.smooth) {
            for v in e.v {
                smooth_at.entry(v).or_default().push(key(e.v[0], e.v[1]));
            }
        }
        let normals: HashMap<FaceId, DVec3> =
            self.faces.keys().filter_map(|f| Some((*f, self.face_normal(*f)?))).collect();
        let mut out = HashMap::new();
        for (v, faces) in &vertex_faces {
            // Union the faces at v that share a smooth edge at v.
            let mut group: HashMap<FaceId, usize> = faces.iter().enumerate().map(|(i, f)| (*f, i)).collect();
            let Some(smooth_here) = smooth_at.get(v) else { continue };
            let mut changed = false;
            for k in smooth_here {
                if let Some(fs) = edge_faces.get(k)
                    && fs.len() == 2
                {
                    let (ga, gb) = (group[&fs[0]], group[&fs[1]]);
                    if ga != gb {
                        for g in group.values_mut() {
                            if *g == gb {
                                *g = ga;
                            }
                        }
                        changed = true;
                    }
                }
            }
            if !changed {
                continue;
            }
            for f in faces {
                let g = group[f];
                let sum: DVec3 = group.iter().filter(|(_, x)| **x == g).filter_map(|(h, _)| normals.get(h)).sum();
                if sum.length() > 1e-12 {
                    out.insert((*f, *v), sum.normalize());
                }
            }
        }
        out
    }
}
