//! Recomputing the faces of one plane from its edges.
//!
//! The edges lying in a plane form a planar graph (lines are split wherever they meet,
//! so edges only touch at vertices). Walking the graph and always turning as far right as
//! possible traces every region: bounded regions come out counter-clockwise, and the
//! outside of each connected piece comes out clockwise. A clockwise outline inside a
//! bounded region of another piece is a hole in that region.
//!
//! Each region then becomes a face if it lies inside a face that was there before (the
//! face was split, merged or got a hole; it keeps its material and facing) or if a newly
//! drawn edge bounds it (a loop was closed). A region whose loops equal an old face's
//! keeps that face untouched, id and all.

use std::collections::{BTreeMap, BTreeSet};

use rysmo_math::{DVec2, DVec3, EPS, Plane, PolygonSide, point_in_polygon_2d, polygon_area_2d};

use crate::ids::{EdgeId, FaceId, VertexId};
use crate::mesh::{Face, Mesh};
use crate::spatial::{Aabb, Bvh};
use crate::view::MeshView;

struct OldFace {
    id: FaceId,
    face: Face,
    /// +1 if the face's normal agrees with the plane's, -1 if it is opposite.
    sign: f64,
    outer: Vec<DVec2>,
    holes: Vec<Vec<DVec2>>,
}

impl OldFace {
    fn contains(&self, p: DVec2) -> bool {
        point_in_polygon_2d(p, &self.outer, 1e-9) == PolygonSide::Inside
            && self.holes.iter().all(|h| point_in_polygon_2d(p, h, 1e-9) == PolygonSide::Outside)
    }
}

struct Region {
    outer: Vec<VertexId>,
    holes: Vec<Vec<VertexId>>,
}

/// Finds what a run of plane rebuilds needs. The edges do not change during the run;
/// the faces do, so faces made along the way are listed in `added`.
#[derive(Default)]
pub(crate) struct RegionIndex {
    view: Option<MeshView>,
    added: Vec<FaceId>,
}

impl RegionIndex {
    pub(crate) fn new(view: MeshView) -> Self {
        RegionIndex { view: Some(view), added: Vec::new() }
    }

    fn edges(&self, mesh: &Mesh, plane: &Plane) -> Vec<EdgeId> {
        self.view.as_ref().map_or_else(|| mesh.edges.keys().copied().collect(), |v| v.edges_near_plane(mesh, plane))
    }

    fn faces(&self, mesh: &Mesh, plane: &Plane, reach: Option<Aabb>) -> Vec<FaceId> {
        let Some(view) = &self.view else { return mesh.faces.keys().copied().collect() };
        let mut found = reach.map_or_else(|| view.faces_near_plane(mesh, plane), |reach| view.faces_in(mesh, reach));
        found.extend(self.added.iter().filter(|id| mesh.faces.contains_key(id)));
        found.sort_unstable();
        found.dedup();
        found
    }

    /// The faces whose loops run along `e`, as `Mesh::faces_of_edge` finds them.
    fn faces_of_edge(&self, mesh: &Mesh, e: EdgeId) -> Vec<FaceId> {
        let Some(view) = &self.view else { return mesh.faces_of_edge(e) };
        let mut found = view.faces_of_edge(mesh, e);
        let Some(edge) = mesh.edges.get(&e) else { return found };
        let k = crate::mesh::key(edge.v[0], edge.v[1]);
        let runs_along = |f: &Face| f.loop_edges().iter().any(|(a, b)| crate::mesh::key(*a, *b) == k);
        found.extend(self.added.iter().filter(|id| mesh.faces.get(id).is_some_and(runs_along)));
        found.sort_unstable();
        found.dedup();
        found
    }

    fn note_face(&mut self, id: FaceId) {
        self.added.push(id);
    }

    /// The edges of `plane` a change at `start` can matter to, and the box around them:
    /// those connected to it, and whatever overlaps them, since that may lie inside one
    /// of their regions or hold them inside one of its own. `None` without a view.
    fn plane_part(&self, mesh: &Mesh, plane: &Plane, start: &[EdgeId]) -> Option<(Vec<EdgeId>, Aabb)> {
        let view = self.view.as_ref()?;
        let in_plane =
            |id: &EdgeId| mesh.edges.get(id).is_some_and(|e| e.v.iter().all(|v| plane.contains(mesh.pos(*v))));
        let mut pending: Vec<EdgeId> = start.iter().copied().filter(|id| in_plane(id)).collect();
        let mut seen = BTreeSet::new();
        let mut area = Aabb::EMPTY;
        loop {
            while let Some(id) = pending.pop() {
                if !seen.insert(id) {
                    continue;
                }
                let (a, b) = mesh.edge_points(id);
                area = area.union(Aabb::of([a, b]));
                let [x, y] = mesh.edges[&id].v;
                let neighbours = mesh.edges_of_vertex(x).into_iter().chain(mesh.edges_of_vertex(y));
                pending.extend(neighbours.filter(|n| !seen.contains(n) && in_plane(n)));
            }
            let reach = area.expanded(EPS * 2.0);
            pending.extend(view.edges_in(mesh, reach).into_iter().filter(|id| !seen.contains(id) && in_plane(id)));
            for face in view.faces_in(mesh, reach).into_iter().chain(self.added.iter().copied()) {
                let Some(f) = mesh.faces.get(&face) else { continue };
                let edges = f.loop_edges().into_iter().filter_map(|(a, b)| mesh.edge_between(a, b));
                pending.extend(edges.filter(|id| !seen.contains(id) && in_plane(id)));
            }
            if pending.is_empty() {
                return Some((seen.into_iter().collect(), reach));
            }
        }
    }
}

impl Mesh {
    /// Recomputes the faces lying in `plane`. `seeds` are edges drawn just now: a region
    /// they bound becomes a face even if no face covered it before. `facing` is the
    /// preferred normal direction for brand new faces with nothing else to go by.
    pub(crate) fn rebuild_plane(&mut self, plane: &Plane, seeds: &BTreeSet<EdgeId>, facing: Option<DVec3>) {
        let mut index = if self.faces.len() > crate::mesh::VIEW_FACES {
            RegionIndex::new(self.view())
        } else {
            RegionIndex::default()
        };
        let start: Vec<_> = seeds.iter().copied().collect();
        self.rebuild_plane_in(plane, seeds, facing, &mut index, &start);
    }

    /// Recomputes the regions of `plane` around `start` after an edit with nothing new
    /// to make a face of.
    pub(crate) fn rebuild_plane_around(&mut self, plane: &Plane, start: &[EdgeId]) {
        let mut index = if self.faces.len() > crate::mesh::VIEW_FACES {
            RegionIndex::new(self.view())
        } else {
            RegionIndex::default()
        };
        self.rebuild_plane_in(plane, &BTreeSet::new(), None, &mut index, start);
    }

    pub(crate) fn rebuild_plane_in(
        &mut self,
        plane: &Plane,
        seeds: &BTreeSet<EdgeId>,
        facing: Option<DVec3>,
        index: &mut RegionIndex,
        start: &[EdgeId],
    ) {
        let in_plane = |m: &Mesh, v: VertexId| plane.contains(m.pos(v));
        let part = if start.is_empty() { None } else { index.plane_part(self, plane, start) };
        let reach = part.as_ref().map(|(_, reach)| *reach);
        let candidates = part.map_or_else(|| index.edges(self, plane), |(edges, _)| edges);
        let edges: Vec<(EdgeId, [VertexId; 2])> = candidates
            .into_iter()
            .map(|id| (id, self.edges[&id].v))
            .filter(|(_, v)| in_plane(self, v[0]) && in_plane(self, v[1]))
            .collect();

        let to2d = |m: &Mesh, v: VertexId| plane.to_2d(m.pos(v));
        let old: Vec<OldFace> = index
            .faces(self, plane, reach)
            .into_iter()
            .map(|id| (id, &self.faces[&id]))
            .filter(|(_, f)| f.loops().all(|l| l.iter().all(|v| in_plane(self, *v))))
            .filter(|(_, f)| reach.is_none_or(|reach| Aabb::of(f.outer.iter().map(|v| self.pos(*v))).overlaps(&reach)))
            .filter_map(|(id, f)| {
                let n = self.face_normal(id)?;
                Some(OldFace {
                    id,
                    face: f.clone(),
                    sign: if n.dot(plane.normal) >= 0.0 { 1.0 } else { -1.0 },
                    outer: f.outer.iter().map(|v| to2d(self, *v)).collect(),
                    holes: f.holes.iter().map(|h| h.iter().map(|v| to2d(self, *v)).collect()).collect(),
                })
            })
            .collect();

        if edges.is_empty() {
            return;
        }

        // Positions and the outgoing edges of each vertex, sorted by angle.
        let mut pts: BTreeMap<VertexId, DVec2> = BTreeMap::new();
        let mut out: BTreeMap<VertexId, Vec<VertexId>> = BTreeMap::new();
        for (_, [a, b]) in &edges {
            pts.entry(*a).or_insert_with(|| to2d(self, *a));
            pts.entry(*b).or_insert_with(|| to2d(self, *b));
            out.entry(*a).or_default().push(*b);
            out.entry(*b).or_default().push(*a);
        }
        for (v, list) in out.iter_mut() {
            let o = pts[v];
            list.sort_by(|x, y| {
                let ax = (pts[x] - o).to_angle();
                let ay = (pts[y] - o).to_angle();
                ax.total_cmp(&ay)
            });
        }

        // Trace every directed edge once.
        let mut visited: BTreeSet<(VertexId, VertexId)> = BTreeSet::new();
        let mut cycles: Vec<Vec<VertexId>> = Vec::new();
        for (_, [a, b]) in &edges {
            for (u, v) in [(*a, *b), (*b, *a)] {
                if visited.contains(&(u, v)) {
                    continue;
                }
                let mut cycle = Vec::new();
                let (mut cu, mut cv) = (u, v);
                loop {
                    if !visited.insert((cu, cv)) {
                        break;
                    }
                    cycle.push(cu);
                    let list = &out[&cv];
                    let k = list.iter().position(|x| *x == cu).expect("edge listed at both ends");
                    let w = list[(k + list.len() - 1) % list.len()];
                    cu = cv;
                    cv = w;
                }
                cycles.push(cycle);
            }
        }

        // Union-find over the vertices, to know which piece each cycle belongs to.
        let mut parent: BTreeMap<VertexId, VertexId> = pts.keys().map(|v| (*v, *v)).collect();
        fn find(p: &mut BTreeMap<VertexId, VertexId>, v: VertexId) -> VertexId {
            let mut r = v;
            while p[&r] != r {
                r = p[&r];
            }
            let mut c = v;
            while p[&c] != r {
                let n = p[&c];
                p.insert(c, r);
                c = n;
            }
            r
        }
        for (_, [a, b]) in &edges {
            let ra = find(&mut parent, *a);
            let rb = find(&mut parent, *b);
            if ra != rb {
                parent.insert(ra, rb);
            }
        }

        let mut bounded: Vec<(Vec<VertexId>, f64, VertexId)> = Vec::new();
        let mut outlines: Vec<(Vec<VertexId>, VertexId)> = Vec::new();
        for c in cycles {
            let c = remove_spikes(c);
            if c.len() < 3 {
                continue;
            }
            let poly: Vec<DVec2> = c.iter().map(|v| pts[v]).collect();
            let area = polygon_area_2d(&poly);
            let comp = find(&mut parent, c[0]);
            if area > 1e-12 {
                bounded.push((c, area, comp));
            } else if area < -1e-12 {
                outlines.push((c, comp));
            }
        }

        // Assign each outline as a hole of the smallest bounded region around it.
        let mut regions: Vec<Region> =
            bounded.iter().map(|(c, _, _)| Region { outer: c.clone(), holes: Vec::new() }).collect();
        let flat = |p: DVec2| DVec3::new(p.x, p.y, 0.0);
        let bounded_tree = Bvh::new(
            bounded
                .iter()
                .enumerate()
                .map(|(i, (c, _, _))| (Aabb::of(c.iter().map(|v| flat(pts[v]))).expanded(EPS), i))
                .collect(),
        );
        for (h, comp) in outlines {
            let probe = pts[&h[0]];
            let mut best: Option<(usize, f64)> = None;
            let mut around = Vec::new();
            bounded_tree.overlapping(Aabb::at(flat(probe)), |i| around.push(*i));
            around.sort_unstable();
            for i in around {
                let (c, area, ccomp) = &bounded[i];
                if *ccomp == comp {
                    continue;
                }
                let poly: Vec<DVec2> = c.iter().map(|v| pts[v]).collect();
                if point_in_polygon_2d(probe, &poly, 1e-9) == PolygonSide::Inside && best.is_none_or(|(_, a)| *area < a)
                {
                    best = Some((i, *area));
                }
            }
            if let Some((i, _)) = best {
                regions[i].holes.push(h);
            }
        }

        // Decide which regions are faces.
        let old_tree = Bvh::new(
            old.iter()
                .enumerate()
                .map(|(i, o)| (Aabb::of(o.outer.iter().map(|p| flat(*p))).expanded(EPS), i))
                .collect(),
        );
        let mut kept: BTreeSet<FaceId> = BTreeSet::new();
        let mut reused: BTreeSet<FaceId> = BTreeSet::new();
        let mut new_faces: Vec<(Option<FaceId>, Face)> = Vec::new();
        for r in &regions {
            let outer2: Vec<DVec2> = r.outer.iter().map(|v| pts[v]).collect();
            let holes2: Vec<Vec<DVec2>> = r.holes.iter().map(|h| h.iter().map(|v| pts[v]).collect()).collect();
            let Some(sample) = sample_point(&outer2, &holes2) else { continue };
            let mut around = Vec::new();
            old_tree.overlapping(Aabb::at(flat(sample)), |i| around.push(*i));
            around.sort_unstable();
            let host = around.into_iter().map(|i| &old[i]).find(|o| o.contains(sample));
            if let Some(h) = host
                && same_loops(&h.face, r)
            {
                kept.insert(h.id);
                continue;
            }
            let touches_seed =
                region_edges(r).any(|(a, b)| self.edge_between(a, b).is_some_and(|e| seeds.contains(&e)));
            let (sign, material, back_material, uv, reuse) = match host {
                Some(h) => (h.sign, h.face.material, h.face.back_material, h.face.uv, Some(h.id)),
                None if touches_seed => (self.new_face_sign(plane, r, facing, index), None, None, None, None),
                None => continue,
            };
            let mut face = Face {
                hidden: host.is_some_and(|h| h.face.hidden),
                outer: r.outer.clone(),
                holes: r.holes.clone(),
                material,
                back_material,
                uv,
            };
            if sign < 0.0 {
                face.reverse();
            }
            let reuse = reuse.filter(|id| !reused.contains(id) && !kept.contains(id));
            if let Some(id) = reuse {
                reused.insert(id);
            }
            new_faces.push((reuse, face));
        }

        for o in &old {
            if !kept.contains(&o.id) {
                self.faces.remove(&o.id);
            }
        }
        for (reuse, face) in new_faces {
            let id = match reuse {
                Some(id) if !self.faces.contains_key(&id) => {
                    self.faces.insert(id, face);
                    id
                }
                _ => self.raw_face(face),
            };
            index.note_face(id);
        }
    }

    /// Which way a brand new face in `plane` should face: consistently with the faces
    /// it shares edges with, then down on the ground, then towards `facing`.
    fn new_face_sign(&self, plane: &Plane, r: &Region, facing: Option<DVec3>, index: &RegionIndex) -> f64 {
        let mut vote = 0i32;
        for (a, b) in region_edges(r) {
            let Some(e) = self.edge_between(a, b) else { continue };
            for f in index.faces_of_edge(self, e) {
                let Some(fp) = self.face_plane(f) else { continue };
                if fp.same_surface(plane) {
                    continue;
                }
                // A neighbour running the same way means this face must turn round.
                for (x, y) in self.faces[&f].loop_edges() {
                    if x == a && y == b {
                        vote -= 1;
                    } else if x == b && y == a {
                        vote += 1;
                    }
                }
            }
        }
        if vote != 0 {
            return vote.signum() as f64;
        }
        if rysmo_math::parallel(plane.normal, DVec3::Z) && plane.d.abs() < EPS {
            return if plane.normal.z > 0.0 { -1.0 } else { 1.0 };
        }
        if let Some(f) = facing {
            let d = f.dot(plane.normal);
            if d.abs() > 1e-9 {
                return d.signum();
            }
        }
        if rysmo_math::parallel(plane.normal, DVec3::Z) {
            return plane.normal.z.signum();
        }
        1.0
    }
}

fn region_edges(r: &Region) -> impl Iterator<Item = (VertexId, VertexId)> + '_ {
    std::iter::once(&r.outer)
        .chain(r.holes.iter())
        .flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()])))
}

/// Drops back-and-forth steps (`a, b, a`) left by edges that dangle into a region.
fn remove_spikes(mut c: Vec<VertexId>) -> Vec<VertexId> {
    loop {
        let n = c.len();
        if n < 3 {
            return if n == 2 { Vec::new() } else { c };
        }
        let mut changed = false;
        for i in 0..n {
            if c[i] == c[(i + 2) % n] {
                // Remove the tip and one copy of its base.
                let tip = (i + 1) % n;
                let base = (i + 2) % n;
                let (first, second) = if tip > base { (tip, base) } else { (base, tip) };
                c.remove(first);
                c.remove(second);
                changed = true;
                break;
            }
        }
        if !changed {
            return c;
        }
    }
}

/// A point strictly inside a region (inside the outline, outside the holes).
fn sample_point(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Option<DVec2> {
    let n = outer.len();
    for scale in [1e-2, 1e-3, 1e-5] {
        for i in 0..n {
            let a = outer[i];
            let b = outer[(i + 1) % n];
            let d = b - a;
            let len = d.length();
            if len < EPS {
                continue;
            }
            // The interior of a counter-clockwise outline is on the left.
            let left = DVec2::new(-d.y, d.x) / len;
            let p = (a + b) * 0.5 + left * (len * scale).max(EPS * 0.1);
            if point_in_polygon_2d(p, outer, 1e-12) == PolygonSide::Inside
                && holes.iter().all(|h| point_in_polygon_2d(p, h, 1e-12) == PolygonSide::Outside)
            {
                return Some(p);
            }
        }
    }
    None
}

/// Rotates a loop to start at its smallest id, for comparing loops whatever their
/// starting point and direction.
pub(crate) fn canonical(l: &[VertexId]) -> Vec<VertexId> {
    let start = (0..l.len()).min_by_key(|i| l[*i]).unwrap_or(0);
    let fwd: Vec<VertexId> = (0..l.len()).map(|i| l[(start + i) % l.len()]).collect();
    let rev: Vec<VertexId> = (0..l.len()).map(|i| l[(start + l.len() - i) % l.len()]).collect();
    fwd.min(rev)
}

fn same_loops(f: &Face, r: &Region) -> bool {
    if canonical(&f.outer) != canonical(&r.outer) || f.holes.len() != r.holes.len() {
        return false;
    }
    let mut a: Vec<_> = f.holes.iter().map(|h| canonical(h)).collect();
    let mut b: Vec<_> = r.holes.iter().map(|h| canonical(h)).collect();
    a.sort();
    b.sort();
    a == b
}
