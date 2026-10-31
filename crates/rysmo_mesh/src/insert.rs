//! Drawing lines into the mesh: splitting at crossings, then rebuilding the faces of
//! every plane the new lines lie in.

use std::collections::{BTreeSet, HashMap};

use rysmo_math::{DVec3, EPS, Plane, closest_on_segment, parallel, segment_intersection};

use crate::ids::{EdgeId, FaceId, VertexId};
use crate::mesh::{Mesh, MeshError};
use crate::spatial::{Aabb, Bvh, DynamicBvh};

/// How far past a box the tolerances can reach.
pub(crate) const MARGIN: f64 = EPS * 2.0;

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
        let mut index = self.line_index(lines);
        for (p, q) in lines {
            new_edges.extend(self.insert_segment(&mut index, *p, *q));
        }
        // Splitting by later lines may have removed earlier edges; follow them by points.
        let mut seeds: Vec<EdgeId> = Vec::new();
        for (p, q) in lines {
            seeds.extend(self.edges_along_in(&index, *p, *q));
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
            .filter(|(_, e)| lies_on(p, q, self.pos(e.v[0])) && lies_on(p, q, self.pos(e.v[1])))
            .map(|(id, _)| *id)
            .collect()
    }

    /// `edges_along` for many segments in one pass over the edges.
    pub fn edges_along_each(&self, segments: &[(DVec3, DVec3)]) -> Vec<Vec<EdgeId>> {
        let tree =
            Bvh::new(segments.iter().enumerate().map(|(i, (p, q))| (Aabb::of([*p, *q]).expanded(MARGIN), i)).collect());
        let mut found = vec![Vec::new(); segments.len()];
        for (id, e) in &self.edges {
            let (a, b) = (self.pos(e.v[0]), self.pos(e.v[1]));
            tree.overlapping(Aabb::of([a, b]), |i| {
                let (p, q) = segments[*i];
                if lies_on(p, q, a) && lies_on(p, q, b) {
                    found[*i].push(*id);
                }
            });
        }
        found
    }

    /// What a batch of insertions along `lines` has to look at: the vertices, edges and
    /// faces near them, gathered in one pass over the mesh.
    pub(crate) fn line_index(&self, lines: &[(DVec3, DVec3)]) -> LineIndex {
        let reach = |a: DVec3, b: DVec3| Aabb::of([a, b]).expanded(MARGIN);
        let near = Bvh::new(lines.iter().map(|(p, q)| (reach(*p, *q), ())).collect());
        let touches = |area: Aabb| near.any(area, |_| true);
        let mut edges = Vec::new();
        let mut around: HashMap<VertexId, Vec<FaceId>> = HashMap::new();
        for (id, edge) in &self.edges {
            let area = reach(self.pos(edge.v[0]), self.pos(edge.v[1]));
            if touches(area) {
                edges.push((area, *id));
                around.extend(edge.v.map(|v| (v, Vec::new())));
            }
        }
        for (id, face) in &self.faces {
            for v in face.loops().flatten() {
                if let Some(faces) = around.get_mut(v) {
                    faces.push(*id);
                }
            }
        }
        let vertices = self
            .vertices
            .iter()
            .map(|(id, v)| (Aabb::at(v.pos).expanded(MARGIN), *id))
            .filter(|(area, _)| touches(*area))
            .collect();
        LineIndex { vertices: DynamicBvh::new(vertices), edges: DynamicBvh::new(edges), around }
    }

    fn vertices_near(&self, index: &LineIndex, area: Aabb) -> Vec<VertexId> {
        let mut found = Vec::new();
        index.vertices.overlapping(area, |id| found.push(*id));
        found.sort_unstable();
        found.dedup();
        found
    }

    /// The edges near `area` that still exist.
    pub(crate) fn edges_near(&self, index: &LineIndex, area: Aabb) -> Vec<EdgeId> {
        let mut found = Vec::new();
        index.edges.overlapping(area, |id| found.push(*id));
        found.retain(|id| self.edges.contains_key(id));
        found.sort_unstable();
        found.dedup();
        found
    }

    fn edges_along_in(&self, index: &LineIndex, p: DVec3, q: DVec3) -> Vec<EdgeId> {
        let mut found = self.edges_near(index, Aabb::of([p, q]).expanded(MARGIN));
        found.retain(|id| {
            let [a, b] = self.edges[id].v;
            lies_on(p, q, self.pos(a)) && lies_on(p, q, self.pos(b))
        });
        found
    }

    pub(crate) fn add_vertex_in(&mut self, index: &mut LineIndex, p: DVec3) -> VertexId {
        let near = self.vertices_near(index, Aabb::at(p).expanded(MARGIN));
        if let Some(v) = near.into_iter().find(|v| self.pos(*v).distance(p) < EPS) {
            return v;
        }
        let v = self.raw_vertex(p);
        index.vertices.insert(Aabb::at(p).expanded(MARGIN), v);
        v
    }

    fn raw_edge_in(&mut self, index: &mut LineIndex, a: VertexId, b: VertexId) -> EdgeId {
        if let Some(e) = self.edge_between(a, b) {
            return e;
        }
        let e = self.raw_edge(a, b);
        index.edges.insert(Aabb::of([self.pos(a), self.pos(b)]).expanded(MARGIN), e);
        e
    }

    pub(crate) fn split_edge_indexed(&mut self, index: &mut LineIndex, e: EdgeId, v: VertexId) -> (EdgeId, EdgeId) {
        let [a, _] = self.edges[&e].v;
        let faces = index.around.get(&a).cloned().unwrap_or_default();
        let ((first, second), threaded) = self.split_edge_in(e, v, &faces);
        index.around.entry(v).or_default().extend(threaded);
        for half in [first, second] {
            let [x, y] = self.edges[&half].v;
            index.edges.insert(Aabb::of([self.pos(x), self.pos(y)]).expanded(MARGIN), half);
        }
        (first, second)
    }

    /// Inserts the segment `p..q` as edges, splitting existing edges where it meets them
    /// and splitting itself at every vertex on it. Faces are threaded with the new split
    /// vertices but not otherwise changed.
    fn insert_segment(&mut self, index: &mut LineIndex, p: DVec3, q: DVec3) -> Vec<EdgeId> {
        let a = self.add_vertex_in(index, p);
        let b = self.add_vertex_in(index, q);
        // The ends may land inside existing edges.
        for v in [a, b] {
            let pos = self.pos(v);
            if let Some(e) = self.edge_through(index, pos, v) {
                self.split_edge_indexed(index, e, v);
            }
        }
        // Crossings with other edges.
        let p = self.pos(a);
        let q = self.pos(b);
        let crossing: Vec<(EdgeId, f64)> = self
            .edges_near(index, Aabb::of([p, q]).expanded(MARGIN))
            .into_iter()
            .filter(|id| !self.edges[id].v.contains(&a) && !self.edges[id].v.contains(&b))
            .filter_map(|id| {
                let e = &self.edges[&id];
                let c = self.pos(e.v[0]);
                let d = self.pos(e.v[1]);
                let (s, t) = segment_intersection(p, q, c, d)?;
                let x = p + (q - p) * s;
                let inner_new = x.distance(p) > EPS && x.distance(q) > EPS;
                let inner_old = x.distance(c) > EPS && x.distance(d) > EPS;
                (inner_new && inner_old).then_some((id, t))
            })
            .collect();
        for (e, t) in crossing {
            let Some(edge) = self.edges.get(&e) else { continue };
            let c = self.pos(edge.v[0]);
            let d = self.pos(edge.v[1]);
            let x = c + (d - c) * t;
            let v = self.add_vertex_in(index, x);
            if !self.edges[&e].v.contains(&v) {
                self.split_edge_indexed(index, e, v);
            }
        }
        // Collinear overlaps: the ends of existing edges lying on p..q split it, and
        // the overlapping parts reuse the existing edges.
        let dir = q - p;
        let len2 = dir.length_squared();
        let mut on_line: Vec<(f64, VertexId)> = self
            .vertices_near(index, Aabb::of([p, q]).expanded(MARGIN))
            .into_iter()
            .filter_map(|id| {
                let at = self.pos(id);
                lies_on(p, q, at).then(|| ((at - p).dot(dir) / len2, id))
            })
            .collect();
        on_line.sort_by(|x, y| x.0.total_cmp(&y.0));
        on_line.dedup_by_key(|x| x.1);
        let mut out = Vec::new();
        for w in on_line.windows(2) {
            if w[0].1 != w[1].1 {
                out.push(self.raw_edge_in(index, w[0].1, w[1].1));
            }
        }
        out
    }

    /// An edge whose interior passes through `p` (not at its ends), other than at `v`.
    fn edge_through(&self, index: &LineIndex, p: DVec3, v: VertexId) -> Option<EdgeId> {
        self.edges_near(index, Aabb::at(p).expanded(MARGIN)).into_iter().find(|id| {
            let e = &self.edges[id];
            if e.v.contains(&v) {
                return false;
            }
            let a = self.pos(e.v[0]);
            let b = self.pos(e.v[1]);
            let (c, _) = closest_on_segment(a, b, p);
            c.distance(p) < EPS && p.distance(a) > EPS && p.distance(b) > EPS
        })
    }

    /// Rebuilds the faces of every plane that contains one of `seeds` together with an
    /// existing face or a neighbouring edge.
    pub(crate) fn rebuild_around(&mut self, seeds: &[EdgeId], facing: Option<DVec3>) {
        let seed_boxes = seeds.iter().filter_map(|e| {
            let [a, b] = self.edges.get(e)?.v;
            Some((Aabb::of([self.pos(a), self.pos(b)]).expanded(MARGIN), *e))
        });
        let near = Bvh::new(seed_boxes.collect());
        // One pass over the faces and edges finds what each seed needs.
        let mut in_plane: HashMap<EdgeId, Vec<FaceId>> = HashMap::new();
        for (id, face) in &self.faces {
            let area = Aabb::of(face.outer.iter().map(|v| self.pos(*v)));
            near.overlapping(area, |seed| in_plane.entry(*seed).or_default().push(*id));
        }
        let mut planes = PlaneSet::default();
        for &e in seeds {
            let Some(edge) = self.edges.get(&e) else { continue };
            let [a, b] = edge.v;
            let pa = self.pos(a);
            let pb = self.pos(b);
            let dir = (pb - pa).normalize();
            // Planes of faces the edge lies in.
            for f in in_plane.get(&e).into_iter().flatten() {
                if let Some(pl) = self.face_plane(*f)
                    && pl.contains(pa)
                    && pl.contains(pb)
                {
                    planes.push(pl);
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
                planes.push(Plane::new(dir.cross(ndir), pa));
            }
        }
        let seed_set: BTreeSet<EdgeId> = seeds.iter().copied().collect();
        let mut index = crate::region::RegionIndex::new(self, planes.list.len());
        for pl in planes.list {
            self.rebuild_plane_in(&pl, &seed_set, facing, &mut index);
        }
    }
}

/// Nearby geometry during a batch of insertions. It only grows; `Mesh` drops what it
/// finds there that has since gone.
pub(crate) struct LineIndex {
    vertices: DynamicBvh<VertexId>,
    edges: DynamicBvh<EdgeId>,
    /// The faces at each nearby vertex.
    around: HashMap<VertexId, Vec<FaceId>>,
}

/// Distinct planes in the order found. Planes that match have nearly the same distance
/// from the origin, so buckets of that distance leave few to compare.
#[derive(Default)]
struct PlaneSet {
    list: Vec<Plane>,
    buckets: HashMap<i64, Vec<usize>>,
}

impl PlaneSet {
    fn push(&mut self, plane: Plane) {
        let bucket = (plane.d.abs() / (EPS * 10.0)).floor() as i64;
        let known = (bucket - 1..=bucket + 1)
            .filter_map(|b| self.buckets.get(&b))
            .flatten()
            .any(|i| self.list[*i].same_surface(&plane));
        if !known {
            self.buckets.entry(bucket).or_default().push(self.list.len());
            self.list.push(plane);
        }
    }
}

fn lies_on(p: DVec3, q: DVec3, a: DVec3) -> bool {
    closest_on_segment(p, q, a).0.distance(a) < EPS
}
