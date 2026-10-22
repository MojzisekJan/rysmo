//! Conservative closed-manifold and triangle-intersection validation for CSG.
use rysmo_math::{DVec3, EPS, Plane, closest_on_segment, segment_intersection};
use rysmo_mesh::{FaceId, Mesh, VertexId, triangulate_face};
use std::collections::{BTreeMap, BTreeSet};

/// An oriented use of an edge by one face of a solid.
#[derive(Clone, Copy)]
struct OrientedFaceEdge {
    face: FaceId,
    start: VertexId,
    end: VertexId,
}

pub(super) struct Shell {
    pub faces: BTreeSet<FaceId>,
    pub volume: f64,
    pub depth: usize,
}
pub(super) struct Analysis {
    pub volume: f64,
    pub shells: Vec<Shell>,
}
pub(super) fn validate(mesh: &Mesh, single: bool) -> Result<f64, String> {
    let result = analyse(mesh)?;
    if single && result.shells.len() != 1 {
        return Err("Operand must contain one connected shell".into());
    }
    Ok(result.volume)
}
pub(super) fn analyse(mesh: &Mesh) -> Result<Analysis, String> {
    mesh.validate().map_err(|e| e.to_string())?;
    if mesh.face_count() == 0 {
        return Err("No closed surface".into());
    }
    let mut edges: BTreeMap<(VertexId, VertexId), Vec<OrientedFaceEdge>> = BTreeMap::new();
    let mut triangles = Vec::new();
    let mut adjacent: BTreeMap<FaceId, BTreeSet<FaceId>> = BTreeMap::new();
    let mut fans: BTreeMap<VertexId, BTreeMap<FaceId, BTreeSet<FaceId>>> = BTreeMap::new();

    for (id, face) in mesh.faces() {
        for (a, b) in face.loop_edges() {
            edges.entry((a.min(b), a.max(b))).or_default().push(OrientedFaceEdge { face: id, start: a, end: b });
        }
        let ts = triangulate_face(mesh, id);
        if ts.is_empty() {
            return Err("Cannot triangulate solid face".into());
        }
        let mut area = 0.;
        for t in ts {
            let p = t.map(|v| mesh.pos(v));
            let cross = (p[1] - p[0]).cross(p[2] - p[0]);
            if !cross.is_finite() {
                return Err("Non-finite solid triangle".into());
            }
            if cross.length() < EPS * EPS {
                continue;
            }
            area += cross.length() * 0.5;
            if !mesh.face_contains(id, (p[0] + p[1] + p[2]) / 3.) {
                return Err("Invalid face boundary or hole".into());
            }
            triangles.push((id, p));
        }
        if area < EPS * EPS {
            return Err("Degenerate solid face".into());
        }
        if (area - mesh.face_area(id)).abs() > area.max(1.) * 1e-7 {
            return Err("Overlapping face loops or invalid triangulation".into());
        }
    }
    if triangles.len() > 8192 {
        return Err("Solid exceeds the triangle validation limit".into());
    }
    for (_, edge) in mesh.edges() {
        let [a, b] = edge.v;
        if edges.get(&(a.min(b), a.max(b))).is_none_or(|s| s.len() != 2) {
            return Err("Open, stray or non-manifold edge".into());
        }
    }
    for steps in edges.values() {
        if steps.len() != 2 {
            return Err("Open or non-manifold surface".into());
        }
        let OrientedFaceEdge { face: f, start: a, end: b } = steps[0];
        let OrientedFaceEdge { face: g, start: c, end: d } = steps[1];
        if f == g || a != d || b != c {
            return Err("Inconsistent face orientation".into());
        }
        adjacent.entry(f).or_default().insert(g);
        adjacent.entry(g).or_default().insert(f);
        for v in [a, b] {
            fans.entry(v).or_default().entry(f).or_default().insert(g);
            fans.entry(v).or_default().entry(g).or_default().insert(f);
        }
    }
    for fan in fans.values() {
        if connected(fan, *fan.keys().next().unwrap()).len() != fan.len() {
            return Err("Non-manifold vertex".into());
        }
    }
    let mut remaining = mesh.faces().map(|(id, _)| id).collect::<BTreeSet<_>>();
    let mut total = 0.;
    let mut shells = Vec::new();
    while let Some(seed) = remaining.first().copied() {
        let shell = connected(&adjacent, seed);
        let origin = mesh.pos(mesh.face(seed).unwrap().outer[0]);
        let mut volume = 0.;
        let mut extent: f64 = 0.;
        for (_, triangle) in triangles.iter().filter(|(id, _)| shell.contains(id)) {
            let [a, b, c] = triangle.map(|p| p - origin);
            extent = extent.max(a.length()).max(b.length()).max(c.length());
            volume += a.dot(b.cross(c)) / 6.;
        }
        if volume.abs() <= extent.powi(3) * 1e-12 || !volume.is_finite() {
            return Err("Degenerate solid volume".into());
        }
        total += volume;
        shells.push(Shell { faces: shell.clone(), volume, depth: 0 });
        for id in shell {
            remaining.remove(&id);
        }
    }
    let mut budget = 2_000_000usize;
    for (i, (face, a)) in triangles.iter().enumerate() {
        for (other, b) in &triangles[i + 1..] {
            if face == other {
                continue;
            }
            budget = budget.checked_sub(1).ok_or("Solid intersection validation exceeds its complexity limit")?;
            if (0..3).any(|axis| {
                let amin = a.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
                let amax = a.iter().map(|p| p[axis]).fold(f64::NEG_INFINITY, f64::max);
                let bmin = b.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
                let bmax = b.iter().map(|p| p[axis]).fold(f64::NEG_INFINITY, f64::max);
                amax < bmin - EPS || bmax < amin - EPS
            }) {
                continue;
            }
            let first = mesh.face(*face).unwrap();
            let second = mesh.face(*other).unwrap();
            let common =
                first.loops().flatten().filter(|id| second.has_vertex(**id)).map(|v| mesh.pos(*v)).collect::<Vec<_>>();
            let shared = first
                .loop_edges()
                .into_iter()
                .filter(|(a, b)| second.loop_edges().iter().any(|(c, d)| (a == c && b == d) || (a == d && b == c)))
                .map(|(a, b)| (mesh.pos(a), mesh.pos(b)))
                .collect::<Vec<_>>();
            for point in intersections(*a, *b) {
                if !common.iter().any(|p| p.distance(point) <= EPS * 2.)
                    && !shared.iter().any(|(a, b)| closest_on_segment(*a, *b, point).0.distance(point) <= EPS * 2.)
                {
                    return Err("Self-intersecting or touching solid faces".into());
                }
            }
        }
    }
    // After rejecting contacts/intersections, a point on one shell is strictly
    // inside or outside every other shell. Solid angles avoid ray/vertex ambiguity.
    let mut depths = vec![0; shells.len()];
    for (i, shell) in shells.iter().enumerate() {
        let sample = mesh.pos(mesh.face(*shell.faces.first().unwrap()).unwrap().outer[0]);
        for (j, other) in shells.iter().enumerate() {
            if i == j {
                continue;
            }
            let mut angle = 0.;
            for (_, triangle) in triangles.iter().filter(|(id, _)| other.faces.contains(id)) {
                budget = budget.checked_sub(1).ok_or("Shell containment exceeds its complexity limit")?;
                let relative = triangle.map(|p| p - sample);
                if relative.iter().any(|p| p.length() < EPS) {
                    return Err("Touching solid shells".into());
                }
                let [a, b, c] = relative.map(|p| p.normalize());
                angle += 2. * a.dot(b.cross(c)).atan2(1. + a.dot(b) + b.dot(c) + c.dot(a));
            }
            let winding = angle.abs() / std::f64::consts::TAU / 2.;
            if winding < 1e-4 {
            } else if (winding - 1.).abs() < 1e-4 {
                depths[i] += 1;
            } else {
                return Err("Ambiguous shell containment".into());
            }
        }
    }
    let mut orientation = None;
    for (shell, depth) in shells.iter_mut().zip(depths) {
        shell.depth = depth;
        let sign = shell.volume.signum() * if depth % 2 == 0 { 1. } else { -1. };
        if orientation.is_some_and(|old| old != sign) {
            return Err("Shell orientation does not match cavity nesting".into());
        }
        orientation = Some(sign);
    }
    if !total.is_finite() || total.signum() != orientation.unwrap_or(0.) {
        return Err("Invalid combined solid volume".into());
    }
    Ok(Analysis { volume: total, shells })
}
fn connected(graph: &BTreeMap<FaceId, BTreeSet<FaceId>>, seed: FaceId) -> BTreeSet<FaceId> {
    let mut seen = BTreeSet::from([seed]);
    let mut stack = vec![seed];
    while let Some(id) = stack.pop() {
        for next in graph.get(&id).into_iter().flatten() {
            if seen.insert(*next) {
                stack.push(*next);
            }
        }
    }
    seen
}
fn contains(t: [DVec3; 3], p: DVec3) -> bool {
    let normal = (t[1] - t[0]).cross(t[2] - t[0]).normalize();
    (0..3).all(|i| {
        let edge = t[(i + 1) % 3] - t[i];
        normal.dot(edge.cross(p - t[i])) >= -EPS * edge.length()
    })
}
fn intersections(a: [DVec3; 3], b: [DVec3; 3]) -> Vec<DVec3> {
    let pa = Plane::new((a[1] - a[0]).cross(a[2] - a[0]), a[0]);
    let pb = Plane::new((b[1] - b[0]).cross(b[2] - b[0]), b[0]);
    let mut out = Vec::new();
    if a.iter().all(|p| pb.distance(*p).abs() <= EPS) && b.iter().all(|p| pa.distance(*p).abs() <= EPS) {
        out.extend(a.into_iter().filter(|p| contains(b, *p)));
        out.extend(b.into_iter().filter(|p| contains(a, *p)));
        for i in 0..3 {
            for j in 0..3 {
                if let Some((s, _)) = segment_intersection(a[i], a[(i + 1) % 3], b[j], b[(j + 1) % 3]) {
                    out.push(a[i] + (a[(i + 1) % 3] - a[i]) * s);
                }
            }
        }
    } else {
        for (triangle, other, plane) in [(a, b, pb), (b, a, pa)] {
            for i in 0..3 {
                let p = triangle[i];
                let q = triangle[(i + 1) % 3];
                let dp = plane.distance(p);
                let dq = plane.distance(q);
                if dp.abs() <= EPS && contains(other, p) {
                    out.push(p);
                }
                if dp * dq < 0. {
                    let point = p + (q - p) * (dp / (dp - dq));
                    if contains(other, point) {
                        out.push(point);
                    }
                }
            }
        }
    }
    out
}
