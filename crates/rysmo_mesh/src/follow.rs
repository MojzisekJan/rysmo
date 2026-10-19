//! Follow Me: sweeping a face along a path of edges.

use std::collections::BTreeSet;

use rysmo_math::{DVec3, EPS};

use crate::ids::{EdgeId, FaceId, VertexId};
use crate::mesh::{Mesh, MeshError};
use crate::region::canonical;

impl Mesh {
    /// Sweeps face `face` along the path `path` (points in order; `closed` joins the last
    /// back to the first), as Follow Me does. Corners are mitred: each
    /// profile point travels along the path's direction to the plane halfway between
    /// two segments. An open path keeps the face as the start cap and closes the end
    /// with a copy; a closed path leaves a ring and removes the face. New faces face
    /// outwards; edges between faces that meet at a shallow angle are softened.
    pub fn follow_me(&mut self, face: FaceId, path: &[DVec3], closed: bool) -> Result<Vec<FaceId>, MeshError> {
        let mut path: Vec<DVec3> = path.to_vec();
        path.dedup_by(|a, b| a.distance(*b) < EPS);
        if closed && path.len() > 1 && path[0].distance(*path.last().expect("not empty")) < EPS {
            path.pop();
        }
        let n = path.len();
        if n < 2 || (closed && n < 3) {
            return Err(MeshError::Unsupported("the path needs at least one segment".into()));
        }
        let profile = self.faces.get(&face).ok_or(MeshError::Missing(face.0))?.clone();
        let loops: Vec<Vec<DVec3>> = profile.loops().map(|l| self.loop_points(l)).collect();
        let boundary = self.face_edges(face);
        let curve_ids: BTreeSet<_> = boundary.iter().filter_map(|e| self.edges[e].curve).collect();
        let profile_curves: Vec<_> = curve_ids
            .iter()
            .map(|id| {
                let all = self.curve_edges(*id);
                let members: Vec<_> = all.iter().copied().filter(|e| boundary.contains(e)).collect();
                let mut curve = self.curves[id];
                if members.len() != all.len() {
                    curve.kind = crate::CurveKind::Polyline;
                }
                (curve, members.iter().map(|e| self.edge_points(*e)).collect::<Vec<_>>())
            })
            .collect();
        let smooth_vertices: BTreeSet<_> = profile
            .loops()
            .flatten()
            .copied()
            .filter(|v| {
                let ids: Vec<_> = boundary
                    .iter()
                    .filter_map(|e| {
                        let edge = &self.edges[e];
                        edge.curve.filter(|id| {
                            edge.v.contains(v)
                                && self
                                    .curves
                                    .get(id)
                                    .is_some_and(|c| !c.faceted && c.kind != crate::CurveKind::Polygon)
                        })
                    })
                    .collect();
                ids.iter().any(|id| ids.iter().filter(|other| *other == id).count() >= 2)
            })
            .collect();
        let smooth_points: BTreeSet<_> = profile
            .loops()
            .enumerate()
            .flat_map(|(l, vertices)| {
                vertices.iter().enumerate().filter(|(_, v)| smooth_vertices.contains(v)).map(move |(k, _)| (l, k))
            })
            .collect();
        let polygon_vertices: BTreeSet<_> = self
            .face_edges(face)
            .iter()
            .filter_map(|e| {
                let edge = &self.edges[e];
                edge.curve
                    .and_then(|id| self.curves.get(&id))
                    .filter(|c| c.faceted || c.kind == crate::CurveKind::Polygon)
                    .map(|_| edge.v)
            })
            .flatten()
            .collect();
        let polygon_points: BTreeSet<_> = profile
            .loops()
            .enumerate()
            .flat_map(|(l, vertices)| {
                vertices.iter().enumerate().filter(|(_, v)| polygon_vertices.contains(v)).map(move |(k, _)| (l, k))
            })
            .collect();
        let centre = loops[0].iter().copied().sum::<DVec3>() / loops[0].len() as f64;
        // Start from the end of an open path nearer the profile. A closed path starts at
        // its vertex nearest the profile, going along whichever of its two segments
        // there runs more nearly through the profile.
        if !closed && path[n - 1].distance(centre) < path[0].distance(centre) {
            path.reverse();
        }
        if closed {
            let normal = self.face_normal(face).unwrap_or(DVec3::Z);
            let k = (0..n).min_by(|a, b| path[*a].distance(centre).total_cmp(&path[*b].distance(centre))).unwrap_or(0);
            path.rotate_left(k);
            let out = (path[1] - path[0]).normalize();
            let back = (path[n - 1] - path[0]).normalize();
            if back.dot(normal).abs() > out.dot(normal).abs() + 1e-9 {
                path[1..].reverse();
            }
        }
        let segs = if closed { n } else { n - 1 };
        let dirs: Vec<DVec3> = (0..segs).map(|i| (path[(i + 1) % n] - path[i]).normalize()).collect();
        let miter = |j: usize| -> DVec3 {
            let (before, after) = if closed {
                (Some(dirs[(j + segs - 1) % segs]), Some(dirs[j % segs]))
            } else {
                (j.checked_sub(1).map(|k| dirs[k]), (j < segs).then(|| dirs[j]))
            };
            match (before, after) {
                (Some(a), Some(b)) if (a + b).length() > 1e-9 => (a + b).normalize(),
                (Some(a), _) => a,
                (_, Some(b)) => b,
                _ => DVec3::Z,
            }
        };
        let to_plane = |q: DVec3, dir: DVec3, at: DVec3, m: DVec3| -> Option<DVec3> {
            let den = dir.dot(m);
            if den.abs() < 1e-9 {
                return None;
            }
            Some(q + dir * ((at - q).dot(m) / den))
        };
        let bad = || MeshError::Unsupported("the path turns back on itself".into());
        // rings[j][loop][k]
        let mut rings: Vec<Vec<Vec<DVec3>>> = Vec::new();
        let start: Vec<Vec<DVec3>> = if closed {
            let m0 = miter(0);
            loops
                .iter()
                .map(|l| l.iter().map(|q| to_plane(*q, dirs[0], path[0], m0).ok_or_else(bad)).collect())
                .collect::<Result<_, _>>()?
        } else {
            loops.clone()
        };
        rings.push(start);
        for j in 0..segs {
            if closed && j == segs - 1 {
                break;
            }
            let at = path[(j + 1) % n];
            let m = miter(j + 1);
            let prev = rings.last().expect("start ring");
            let next: Vec<Vec<DVec3>> = prev
                .iter()
                .map(|l| l.iter().map(|q| to_plane(*q, dirs[j], at, m).ok_or_else(bad)).collect())
                .collect::<Result<_, _>>()?;
            rings.push(next);
        }
        let mut lines: Vec<(DVec3, DVec3)> = Vec::new();
        let ring_count = rings.len();
        let first_drawn = if closed { 0 } else { 1 };
        for ring in rings.iter().skip(first_drawn) {
            for l in ring {
                for k in 0..l.len() {
                    lines.push((l[k], l[(k + 1) % l.len()]));
                }
            }
        }
        let links = if closed { ring_count } else { ring_count - 1 };
        for j in 0..links {
            let (a, b) = (&rings[j], &rings[(j + 1) % ring_count]);
            for (la, lb) in a.iter().zip(b) {
                for (p, q) in la.iter().zip(lb) {
                    lines.push((*p, *q));
                }
            }
        }
        lines.retain(|(a, b)| a.distance(*b) >= EPS);
        let before: BTreeSet<FaceId> = self.faces.keys().copied().collect();
        // A closed sweep replaces the profile, which may sit off the mitre plane: take it
        // away first so its edges do not cut the new faces.
        if closed {
            let profile_edges = self.face_edges(face);
            self.faces.remove(&face);
            for e in profile_edges {
                if self.faces_of_edge(e).is_empty() {
                    self.remove_edge_raw(e);
                }
            }
            self.remove_orphan_vertices();
        }
        self.add_lines(&lines, None)?;
        for j in first_drawn..ring_count {
            let map_point = |mut p: DVec3| {
                if closed {
                    p = to_plane(p, dirs[0], path[0], miter(0)).expect("validated mitre");
                }
                for k in 0..j {
                    p = to_plane(p, dirs[k], path[(k + 1) % n], miter(k + 1)).expect("validated mitre");
                }
                p
            };
            for (curve, lines) in &profile_curves {
                let mapped: Vec<_> = lines.iter().map(|(a, b)| (map_point(*a), map_point(*b))).collect();
                let edges = self.edges_on_segments(&mapped);
                if !edges.is_empty() {
                    self.register_curve(&edges, curve.mapped(&map_point))?;
                }
            }
        }
        // Rings inside the sweep close planar loops, which made faces inside the solid.
        let inner: Vec<usize> = if closed { (0..ring_count).collect() } else { (1..ring_count - 1).collect() };
        for j in inner {
            for l in &rings[j] {
                let ids: Option<Vec<VertexId>> = l.iter().map(|p| self.find_vertex(*p)).collect();
                let Some(ids) = ids else { continue };
                let want = canonical(&ids);
                let doomed: Vec<FaceId> =
                    self.faces.iter().filter(|(_, f)| canonical(&f.outer) == want).map(|(id, _)| *id).collect();
                for f in doomed {
                    self.faces.remove(&f);
                }
            }
        }

        // Lines close other planar loops too (the courtyard of a ring); only the side
        // quads and the end cap are part of the sweep.
        let mut expected: Vec<Vec<DVec3>> = Vec::new();
        for j in 0..links {
            let (a, b) = (&rings[j], &rings[(j + 1) % ring_count]);
            for (la, lb) in a.iter().zip(b) {
                for k in 0..la.len() {
                    let k2 = (k + 1) % la.len();
                    expected.push(vec![la[k], la[k2], lb[k2], lb[k]]);
                }
            }
        }
        if !closed {
            expected.push(rings[ring_count - 1][0].clone());
        }
        let inside_expected = |m: &Mesh, f: FaceId| {
            let Some(t) = crate::triangulate_face(m, f).into_iter().next() else { return false };
            let c = (m.pos(t[0]) + m.pos(t[1]) + m.pos(t[2])) / 3.0;
            expected.iter().any(|poly| {
                let Some(plane) = rysmo_math::Plane::from_polygon(poly) else { return false };
                if !plane.contains(c) {
                    return false;
                }
                let p2: Vec<_> = poly.iter().map(|p| plane.to_2d(*p)).collect();
                rysmo_math::point_in_polygon_2d(plane.to_2d(c), &p2, 1e-9) != rysmo_math::PolygonSide::Outside
            })
        };
        let strays: Vec<FaceId> =
            self.faces.keys().copied().filter(|f| !before.contains(f) && !inside_expected(self, *f)).collect();
        for f in strays {
            self.faces.remove(&f);
        }
        let mut swept: BTreeSet<FaceId> = self.faces.keys().copied().filter(|f| !before.contains(f)).collect();
        if !closed && self.faces.contains_key(&face) {
            swept.insert(face);
        }
        let Some(&seed) = swept.iter().next() else {
            return Err(MeshError::Unsupported("the sweep made no faces".into()));
        };
        self.orient_from(seed, &swept);
        if self.signed_volume(&swept) < 0.0 {
            for f in &swept {
                self.faces.get_mut(f).expect("listed").reverse();
            }
        }
        let edges: Vec<EdgeId> =
            swept.iter().flat_map(|f| self.face_edges(*f)).collect::<BTreeSet<_>>().into_iter().collect();
        let mut faceted = BTreeSet::new();
        for j in 0..links {
            for (l, k) in &polygon_points {
                if let (Some(a), Some(b)) =
                    (self.find_vertex(rings[j][*l][*k]), self.find_vertex(rings[(j + 1) % ring_count][*l][*k]))
                    && let Some(edge) = self.edge_between(a, b)
                {
                    faceted.insert(edge);
                }
            }
        }
        let smooth: Vec<_> = edges.into_iter().filter(|id| !faceted.contains(id)).collect();
        self.soften_by_angle(&smooth, crate::SOFTEN_ANGLE_DEG);
        let mut curved = BTreeSet::new();
        for j in 0..links {
            for (l, k) in &smooth_points {
                if let (Some(a), Some(b)) =
                    (self.find_vertex(rings[j][*l][*k]), self.find_vertex(rings[(j + 1) % ring_count][*l][*k]))
                    && let Some(edge) = self.edge_between(a, b)
                {
                    curved.insert(edge);
                }
            }
        }
        self.set_edge_flags(&curved.into_iter().collect::<Vec<_>>(), Some(true), Some(true), None);
        self.prune_curves();
        Ok(swept.into_iter().collect())
    }
}
