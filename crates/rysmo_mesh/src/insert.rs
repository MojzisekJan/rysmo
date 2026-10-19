//! Drawing lines into the mesh: splitting at crossings, then rebuilding the faces of
//! every plane the new lines lie in.

use std::collections::BTreeSet;

use rysmo_math::{DVec3, EPS, Plane, closest_on_segment, parallel, segment_intersection};

use crate::ids::{EdgeId, VertexId};
use crate::mesh::{Mesh, MeshError};

impl Mesh {
    /// Draws a line from `p` to `q`, as the Line tool does: it sticks to existing
    /// geometry, splits what it crosses, and creates or splits faces. `facing` is a
    /// direction new faces should face if nothing else decides (usually towards the
    /// camera). Returns the edges the line now consists of.
    pub fn add_line(&mut self, p: DVec3, q: DVec3, facing: Option<DVec3>) -> Result<Vec<EdgeId>, MeshError> {
        self.add_lines(&[(p, q)], facing)
    }

    /// Draws several lines and rebuilds faces once at the end, so a rectangle or circle
    /// gives one face rather than a sequence of intermediate states.
    pub fn add_lines(&mut self, lines: &[(DVec3, DVec3)], facing: Option<DVec3>) -> Result<Vec<EdgeId>, MeshError> {
        let mut new_edges = Vec::new();
        for (p, q) in lines {
            if p.distance(*q) < EPS {
                return Err(MeshError::ZeroLength);
            }
        }
        for (p, q) in lines {
            new_edges.extend(self.insert_segment(*p, *q));
        }
        // Splitting by later lines may have removed earlier edges; follow them by points.
        let mut seeds: Vec<EdgeId> = Vec::new();
        for (p, q) in lines {
            seeds.extend(self.edges_along(*p, *q));
        }
        seeds.sort();
        seeds.dedup();
        self.rebuild_around(&seeds, facing);
        Ok(seeds)
    }

    /// The edges lying on the segment `p..q`.
    pub fn edges_along(&self, p: DVec3, q: DVec3) -> Vec<EdgeId> {
        self.edges
            .iter()
            .filter(|(_, e)| {
                let a = self.pos(e.v[0]);
                let b = self.pos(e.v[1]);
                closest_on_segment(p, q, a).0.distance(a) < EPS && closest_on_segment(p, q, b).0.distance(b) < EPS
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Inserts the segment `p..q` as edges, splitting existing edges where it meets them
    /// and splitting itself at every vertex on it. Faces are threaded with the new split
    /// vertices but not otherwise changed.
    pub(crate) fn insert_segment(&mut self, p: DVec3, q: DVec3) -> Vec<EdgeId> {
        let a = self.add_vertex(p);
        let b = self.add_vertex(q);
        // The ends may land inside existing edges.
        for v in [a, b] {
            let pos = self.pos(v);
            if let Some(e) = self.edge_through(pos, v) {
                self.split_edge(e, v);
            }
        }
        // Crossings with other edges.
        let p = self.pos(a);
        let q = self.pos(b);
        let crossing: Vec<(EdgeId, f64)> = self
            .edges
            .iter()
            .filter(|(_, e)| !e.v.contains(&a) && !e.v.contains(&b))
            .filter_map(|(id, e)| {
                let c = self.pos(e.v[0]);
                let d = self.pos(e.v[1]);
                let (s, t) = segment_intersection(p, q, c, d)?;
                let x = p + (q - p) * s;
                let inner_new = x.distance(p) > EPS && x.distance(q) > EPS;
                let inner_old = x.distance(c) > EPS && x.distance(d) > EPS;
                (inner_new && inner_old).then_some((*id, t))
            })
            .collect();
        for (e, t) in crossing {
            let Some(edge) = self.edges.get(&e) else { continue };
            let c = self.pos(edge.v[0]);
            let d = self.pos(edge.v[1]);
            let x = c + (d - c) * t;
            let v = self.add_vertex(x);
            if !self.edges[&e].v.contains(&v) {
                self.split_edge(e, v);
            }
        }
        // Collinear overlaps: the ends of existing edges lying on p..q split it, and
        // the overlapping parts reuse the existing edges.
        let dir = q - p;
        let len2 = dir.length_squared();
        let mut on_line: Vec<(f64, VertexId)> = self
            .vertices
            .iter()
            .filter_map(|(id, v)| {
                let (c, _) = closest_on_segment(p, q, v.pos);
                (c.distance(v.pos) < EPS).then(|| ((v.pos - p).dot(dir) / len2, *id))
            })
            .collect();
        on_line.sort_by(|x, y| x.0.total_cmp(&y.0));
        on_line.dedup_by_key(|x| x.1);
        let mut out = Vec::new();
        for w in on_line.windows(2) {
            if w[0].1 != w[1].1 {
                out.push(self.raw_edge(w[0].1, w[1].1));
            }
        }
        out
    }

    /// An edge whose interior passes through `p` (not at its ends), other than at `v`.
    fn edge_through(&self, p: DVec3, v: VertexId) -> Option<EdgeId> {
        self.edges
            .iter()
            .find(|(_, e)| {
                if e.v.contains(&v) {
                    return false;
                }
                let a = self.pos(e.v[0]);
                let b = self.pos(e.v[1]);
                let (c, _) = closest_on_segment(a, b, p);
                c.distance(p) < EPS && p.distance(a) > EPS && p.distance(b) > EPS
            })
            .map(|(id, _)| *id)
    }

    /// Rebuilds the faces of every plane that contains one of `seeds` together with an
    /// existing face or a neighbouring edge.
    pub(crate) fn rebuild_around(&mut self, seeds: &[EdgeId], facing: Option<DVec3>) {
        let mut planes: Vec<Plane> = Vec::new();
        let push = |planes: &mut Vec<Plane>, pl: Plane| {
            if !planes.iter().any(|x| x.same_surface(&pl)) {
                planes.push(pl);
            }
        };
        for &e in seeds {
            let Some(edge) = self.edges.get(&e) else { continue };
            let [a, b] = edge.v;
            let pa = self.pos(a);
            let pb = self.pos(b);
            let dir = (pb - pa).normalize();
            // Planes of faces the edge lies in.
            for f in self.faces.keys() {
                if let Some(pl) = self.face_plane(*f)
                    && pl.contains(pa)
                    && pl.contains(pb)
                {
                    push(&mut planes, pl);
                }
            }
            // Planes spanned with each edge at either end.
            let mut neighbours = BTreeSet::new();
            for v in [a, b] {
                for n in self.edges_of_vertex(v) {
                    if n != e {
                        neighbours.insert(n);
                    }
                }
            }
            for n in neighbours {
                let (c, d) = self.edge_points(n);
                let other = if c.distance(pa) < EPS || c.distance(pb) < EPS { d } else { c };
                let ndir = (other - pa).normalize();
                if parallel(dir, ndir) {
                    continue;
                }
                push(&mut planes, Plane::new(dir.cross(ndir), pa));
            }
        }
        let seed_set: BTreeSet<EdgeId> = seeds.iter().copied().collect();
        for pl in planes {
            self.rebuild_plane(&pl, &seed_set, facing);
        }
    }
}
