//! Offset: parallel copies of a face's outline or of a chain of edges, at a distance,
//! in their plane. Also orders edges into chains, which Follow Me uses for its path.

use std::collections::{BTreeMap, BTreeSet};

use rysmo_math::{DVec2, DVec3, EPS, Plane, PolygonSide, point_in_polygon_2d, polygon_area_2d};

use crate::ids::{EdgeId, FaceId, VertexId};
use crate::mesh::{Mesh, MeshError};

/// Offsets a polyline lying in the plane with normal `normal`. Positive distances go to
/// the left of the direction of travel (seen from the side the normal points to); for a
/// counter-clockwise loop that is inwards. Corners are mitred. Returns `None` if the
/// result folds over itself (the distance is too large for the shape).
pub fn offset_polyline(points: &[DVec3], normal: DVec3, distance: f64, closed: bool) -> Option<Vec<DVec3>> {
    let n = points.len();
    if n < 2 || (closed && n < 3) {
        return None;
    }
    let plane = Plane::new(normal, points[0]);
    let (u, v) = plane.basis();
    let to2 = |p: DVec3| DVec2::new(u.dot(p), v.dot(p));
    let to3 = |q: DVec2| u * q.x + v * q.y + plane.normal * plane.d;
    let pts: Vec<DVec2> = points.iter().map(|p| to2(*p)).collect();
    let seg_count = if closed { n } else { n - 1 };
    let dirs: Vec<DVec2> = (0..seg_count).map(|i| (pts[(i + 1) % n] - pts[i]).normalize_or_zero()).collect();
    if dirs.iter().any(|d| d.length() < 0.5) {
        return None;
    }
    let left = |d: DVec2| DVec2::new(-d.y, d.x);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let prev = if closed { Some((i + seg_count - 1) % seg_count) } else { i.checked_sub(1) };
        let next = if closed || i < seg_count { Some(i % seg_count) } else { None };
        let q = match (prev, next) {
            (Some(a), Some(b)) => {
                let (da, db) = (dirs[a], dirs[b]);
                let pa = pts[i] + left(da) * distance;
                let pb = pts[i] + left(db) * distance;
                let cross = da.perp_dot(db);
                if cross.abs() < 1e-9 {
                    pb
                } else {
                    // Where the two shifted lines meet.
                    let t = (pb - pa).perp_dot(db) / cross;
                    pa + da * t
                }
            }
            (None, Some(b)) => pts[i] + left(dirs[b]) * distance,
            (Some(a), None) => pts[i] + left(dirs[a]) * distance,
            (None, None) => return None,
        };
        out.push(q);
    }
    if closed {
        let before = polygon_area_2d(&pts);
        let after = polygon_area_2d(&out);
        if before.signum() != after.signum() || after.abs() < EPS * EPS {
            return None;
        }
    }
    if self_intersects(&out, closed) {
        return None;
    }
    // Every offset segment must keep its direction; a reversed one means it collapsed.
    for i in 0..seg_count {
        let d = out[(i + 1) % n] - out[i];
        if d.dot(dirs[i]) <= EPS {
            return None;
        }
    }
    Some(out.into_iter().map(to3).collect())
}

fn self_intersects(p: &[DVec2], closed: bool) -> bool {
    let n = p.len();
    let segs = if closed { n } else { n - 1 };
    let orient = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
    let cross = |a: DVec2, b: DVec2, c: DVec2, d: DVec2| {
        let (d1, d2, d3, d4) = (orient(c, d, a), orient(c, d, b), orient(a, b, c), orient(a, b, d));
        d1 * d2 < -1e-18 && d3 * d4 < -1e-18
    };
    for i in 0..segs {
        for j in i + 1..segs {
            let adjacent = j == i + 1 || (closed && i == 0 && j == segs - 1);
            if !adjacent && cross(p[i], p[(i + 1) % n], p[j], p[(j + 1) % n]) {
                return true;
            }
        }
    }
    false
}

impl Mesh {
    /// The points an Offset of the face's outline by `distance` would draw (positive
    /// inwards), or `None` if it does not fit.
    pub fn offset_face_points(&self, f: FaceId, distance: f64) -> Option<Vec<DVec3>> {
        self.offset_face_loops(f, distance)?.into_iter().next()
    }

    /// Offset all face boundaries. Holes expand when the outer boundary shrinks.
    /// Reject touching/crossing boundaries before any mesh mutation.
    pub fn offset_face_loops(&self, f: FaceId, distance: f64) -> Option<Vec<Vec<DVec3>>> {
        let plane = self.face_plane(f)?;
        let face = self.face(f)?;
        let loops: Vec<Vec<DVec3>> = face
            .loops()
            .map(|ring| offset_polyline(&self.loop_points(ring), plane.normal, distance, true))
            .collect::<Option<_>>()?;
        let flat: Vec<Vec<DVec2>> = loops.iter().map(|ring| ring.iter().map(|p| plane.to_2d(*p)).collect()).collect();
        for hole in flat.iter().skip(1) {
            if hole.iter().any(|p| point_in_polygon_2d(*p, &flat[0], EPS) != PolygonSide::Inside) {
                return None;
            }
        }
        for i in 0..flat.len() {
            for j in i + 1..flat.len() {
                for a in 0..flat[i].len() {
                    for b in 0..flat[j].len() {
                        let (p, q) = (flat[i][a], flat[i][(a + 1) % flat[i].len()]);
                        let (r, s) = (flat[j][b], flat[j][(b + 1) % flat[j].len()]);
                        let near = [
                            rysmo_math::distance_point_segment_2d(p, r, s),
                            rysmo_math::distance_point_segment_2d(q, r, s),
                            rysmo_math::distance_point_segment_2d(r, p, q),
                            rysmo_math::distance_point_segment_2d(s, p, q),
                        ]
                        .into_iter()
                        .any(|d| d <= EPS);
                        let cross = (q - p).perp_dot(r - p) * (q - p).perp_dot(s - p) < 0.
                            && (s - r).perp_dot(p - r) * (s - r).perp_dot(q - r) < 0.;
                        if near || cross {
                            return None;
                        }
                    }
                }
                if i > 0
                    && (point_in_polygon_2d(flat[i][0], &flat[j], EPS) != PolygonSide::Outside
                        || point_in_polygon_2d(flat[j][0], &flat[i], EPS) != PolygonSide::Outside)
                {
                    return None;
                }
            }
        }
        Some(loops)
    }

    /// Draw all parallel boundaries, splitting the face or adding exterior rings.
    pub fn offset_face(&mut self, f: FaceId, distance: f64) -> Result<Vec<EdgeId>, MeshError> {
        if distance.abs() < EPS {
            return Err(MeshError::ZeroDistance);
        }
        let loops = self.offset_face_loops(f, distance).ok_or(MeshError::OffsetTooLarge)?;
        let lines: Vec<_> =
            loops.iter().flat_map(|pts| (0..pts.len()).map(move |i| (pts[i], pts[(i + 1) % pts.len()]))).collect();
        self.add_lines(&lines, None)
    }

    /// Orders edges into one chain: the vertices in order and whether it closes on
    /// itself. `None` if the edges branch or fall apart.
    pub fn edge_chain(&self, edges: &[EdgeId]) -> Option<(Vec<VertexId>, bool)> {
        let set: BTreeSet<EdgeId> = edges.iter().copied().filter(|e| self.edges.contains_key(e)).collect();
        if set.is_empty() {
            return None;
        }
        let mut adj: BTreeMap<VertexId, Vec<VertexId>> = BTreeMap::new();
        for e in &set {
            let [a, b] = self.edges[e].v;
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
        if adj.values().any(|n| n.len() > 2) {
            return None;
        }
        let ends: Vec<VertexId> = adj.iter().filter(|(_, n)| n.len() == 1).map(|(v, _)| *v).collect();
        let closed = ends.is_empty();
        let start = if closed { *adj.keys().next()? } else { ends[0] };
        let mut chain = vec![start];
        let mut prev: Option<VertexId> = None;
        let mut cur = start;
        while let Some(next) = adj[&cur].iter().copied().find(|x| Some(*x) != prev) {
            if chain.contains(&next) {
                break;
            }
            chain.push(next);
            prev = Some(cur);
            cur = next;
        }
        (chain.len() == adj.len()).then_some((chain, closed))
    }

    /// Offsets a chain of coplanar edges by `distance` to the left of its direction of
    /// travel, seen from `normal`.
    pub fn offset_chain(&mut self, edges: &[EdgeId], normal: DVec3, distance: f64) -> Result<Vec<EdgeId>, MeshError> {
        if distance.abs() < EPS {
            return Err(MeshError::ZeroDistance);
        }
        let (chain, closed) = self.edge_chain(edges).ok_or(MeshError::NotAChain)?;
        let pts = self.loop_points(&chain);
        let pts = offset_polyline(&pts, normal, distance, closed).ok_or(MeshError::OffsetTooLarge)?;
        let n = pts.len();
        let segs = if closed { n } else { n - 1 };
        let lines: Vec<_> = (0..segs).map(|i| (pts[i], pts[(i + 1) % n])).collect();
        self.add_lines(&lines, None)
    }
}
