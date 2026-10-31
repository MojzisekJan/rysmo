//! Drawing lines into the mesh: splitting at crossings, then rebuilding the faces of
//! every plane the new lines lie in.

use std::collections::{BTreeSet, HashMap};

use rysmo_math::{DVec3, EPS, Plane, closest_on_segment, parallel, segment_intersection};

use crate::ids::{EdgeId, FaceId, VertexId};
use crate::mesh::{Mesh, MeshError};
use crate::spatial::{Aabb, DynamicBvh};

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
        let view = self.view();
        let along = |(p, q): &(DVec3, DVec3)| {
            let mut found = view.edges_in(self, Aabb::of([*p, *q]).expanded(MARGIN));
            found.retain(|id| {
                let (a, b) = self.edge_points(*id);
                lies_on(*p, *q, a) && lies_on(*p, *q, b)
            });
            found
        };
        segments.iter().map(along).collect()
    }

    /// What a batch of insertions along `lines` has to look at.
    pub(crate) fn line_index(&self, lines: &[(DVec3, DVec3)]) -> LineIndex {
        let view = self.view();
        let reach = |a: DVec3, b: DVec3| Aabb::of([a, b]).expanded(MARGIN);
        let mut edges = BTreeSet::new();
        let mut vertices = BTreeSet::new();
        for (p, q) in lines {
            edges.extend(view.edges_in(self, reach(*p, *q)));
            vertices.extend(view.vertices_in(self, reach(*p, *q)));
        }
        let mut around: HashMap<VertexId, Vec<FaceId>> = HashMap::new();
        let edges: Vec<_> = edges
            .into_iter()
            .map(|id| {
                let [a, b] = self.edges[&id].v;
                for v in [a, b] {
                    around.entry(v).or_insert_with(|| view.faces_at(self, v));
                }
                (reach(self.pos(a), self.pos(b)), id)
            })
            .collect();
        let vertices = vertices.into_iter().map(|id| (Aabb::at(self.pos(id)).expanded(MARGIN), id)).collect();
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
        let view = self.view();
        let mut planes = PlaneSet::default();
        for &e in seeds {
            let Some(edge) = self.edges.get(&e) else { continue };
            let [a, b] = edge.v;
            let pa = self.pos(a);
            let pb = self.pos(b);
            let dir = (pb - pa).normalize();
            // Planes of faces the edge lies in.
            for f in view.faces_in(self, Aabb::of([pa, pb]).expanded(MARGIN)) {
                if let Some(pl) = self.face_plane(f)
                    && pl.contains(pa)
                    && pl.contains(pb)
                {
                    planes.push(pl, e);
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
                planes.push(Plane::new(dir.cross(ndir), pa), e);
            }
        }
        let seed_set: BTreeSet<EdgeId> = seeds.iter().copied().collect();
        let mut index = crate::region::RegionIndex::new(view);
        for (pl, start) in planes.list.into_iter().zip(planes.seeds) {
            self.rebuild_plane_in(&pl, &seed_set, facing, &mut index, &start);
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
    /// The seeds that led to each plane.
    seeds: Vec<Vec<EdgeId>>,
    buckets: HashMap<i64, Vec<usize>>,
}

impl PlaneSet {
    fn push(&mut self, plane: Plane, seed: EdgeId) {
        let bucket = (plane.d.abs() / (EPS * 10.0)).floor() as i64;
        let known = (bucket - 1..=bucket + 1)
            .filter_map(|b| self.buckets.get(&b))
            .flatten()
            .copied()
            .find(|i| self.list[*i].same_surface(&plane));
        match known {
            Some(i) => self.seeds[i].push(seed),
            None => {
                self.buckets.entry(bucket).or_default().push(self.list.len());
                self.list.push(plane);
                self.seeds.push(vec![seed]);
            }
        }
    }
}

fn lies_on(p: DVec3, q: DVec3, a: DVec3) -> bool {
    closest_on_segment(p, q, a).0.distance(a) < EPS
}
