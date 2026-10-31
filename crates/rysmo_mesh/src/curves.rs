//! Curve entities retain their analytic parameters alongside the sticky edge mesh.
use crate::{CurveId, EdgeId, Mesh, MeshError, VertexId};
use rysmo_math::{DVec3, EPS};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveKind {
    Circle,
    Polygon,
    Arc,
    Polyline,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub kind: CurveKind,
    #[serde(default)]
    pub faceted: bool,
    pub centre: DVec3,
    pub normal: DVec3,
    /// Direction from centre to the first vertex.
    pub start: DVec3,
    pub radius: f64,
    pub segments: u32,
    /// Signed sweep in degrees; circles/polygons use 360.
    pub sweep: f64,
}

impl Curve {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.centre.is_finite()
            || !self.normal.is_finite()
            || !self.start.is_finite()
            || !self.radius.is_finite()
            || !self.sweep.is_finite()
            || self.segments == 0
        {
            return Err("non-finite or empty curve");
        }
        if self.kind != CurveKind::Polyline
            && (self.radius <= EPS
                || self.normal.length() < EPS
                || self.start.length() < EPS
                || self.normal.normalize().dot(self.start.normalize()).abs() > 1e-6)
        {
            return Err("invalid radial curve");
        }
        if matches!(self.kind, CurveKind::Circle | CurveKind::Polygon) && self.segments < 3 {
            return Err("closed curve needs at least three segments");
        }
        Ok(())
    }

    /// Preserve analytic geometry only under uniform scaling in its own plane.
    pub fn mapped(self, f: &impl Fn(DVec3) -> DVec3) -> Self {
        let centre = f(self.centre);
        let u = self.start.normalize_or_zero();
        let v = self.normal.cross(u).normalize_or_zero();
        let a = f(self.centre + u * self.radius) - centre;
        let b = f(self.centre + v * self.radius) - centre;
        let radial = a.length() > EPS
            && b.length() > EPS
            && (a.length() - b.length()).abs() < EPS * a.length().max(1.0)
            && a.normalize().dot(b.normalize()).abs() < 1e-6;
        if self.kind == CurveKind::Polyline || !radial {
            Self { kind: CurveKind::Polyline, centre, ..self }
        } else {
            Self { centre, start: a.normalize(), normal: a.cross(b).normalize(), radius: a.length(), ..self }
        }
    }
}

impl Mesh {
    pub fn curves(&self) -> impl Iterator<Item = (CurveId, &Curve)> {
        self.curves.iter().map(|(id, c)| (*id, c))
    }
    pub fn curve(&self, id: CurveId) -> Option<&Curve> {
        self.curves.get(&id)
    }
    pub fn curve_edges(&self, id: CurveId) -> Vec<EdgeId> {
        if self.edges.len() > crate::mesh::VIEW_FACES {
            return self.view().curve_edges(self, id);
        }
        self.edges.iter().filter_map(|(id_e, e)| (e.curve == Some(id)).then_some(*id_e)).collect()
    }
    /// The edges of every curve, in one pass over the edges.
    pub(crate) fn curve_members(&self) -> BTreeMap<CurveId, Vec<EdgeId>> {
        let mut members: BTreeMap<_, Vec<_>> = BTreeMap::new();
        if !self.curves.is_empty() {
            for (id, edge) in &self.edges {
                if let Some(curve) = edge.curve {
                    members.entry(curve).or_default().push(*id);
                }
            }
        }
        members
    }
    pub fn register_curve(&mut self, edges: &[EdgeId], curve: Curve) -> Result<CurveId, MeshError> {
        let id = self.register_curve_unpruned(edges, curve)?;
        self.prune_curves();
        Ok(id)
    }
    /// `register_curve` for callers adding many curves, which prune once at the end.
    pub(crate) fn register_curve_unpruned(&mut self, edges: &[EdgeId], curve: Curve) -> Result<CurveId, MeshError> {
        curve.validate().map_err(|e| MeshError::Invalid(e.into()))?;
        if edges.is_empty() || edges.iter().any(|id| !self.edges.contains_key(id)) {
            return Err(MeshError::Invalid("curve needs existing edges".into()));
        }
        let id = CurveId(self.reserve_id());
        self.curves.insert(id, curve);
        for edge in edges {
            self.edges.get_mut(edge).unwrap().curve = Some(id);
        }
        Ok(id)
    }
    pub fn set_curve_radius(&mut self, id: CurveId, radius: f64) -> Result<(), MeshError> {
        if !radius.is_finite() || radius <= EPS {
            return Err(MeshError::Invalid("curve radius must be positive and finite".into()));
        }
        let curve = *self.curves.get(&id).ok_or(MeshError::Missing(id.0))?;
        if curve.kind == CurveKind::Polyline {
            return Err(MeshError::Unsupported("a deformed curve has no editable radius".into()));
        }
        let edges = self.curve_edges(id);
        if edges.is_empty() {
            return Err(MeshError::Missing(id.0));
        }
        let mut vertices: BTreeSet<_> = edges.iter().flat_map(|e| self.edges[e].v).collect();
        let factor = radius / curve.radius;
        let normal = curve.normal.normalize();
        // Extruded curve surfaces retain their radius across both rims. Follow only
        // axial surface edges, never unrelated coaxial geometry or loose wires.
        loop {
            let attached: Vec<_> = self
                .edges
                .iter()
                .filter_map(|(id, edge)| {
                    if vertices.contains(&edge.v[0]) == vertices.contains(&edge.v[1])
                        || self.faces_of_edge(*id).len() < 2
                    {
                        return None;
                    }
                    let delta = self.pos(edge.v[1]) - self.pos(edge.v[0]);
                    if delta.cross(normal).length() > EPS {
                        return None;
                    }
                    Some(if vertices.contains(&edge.v[0]) { edge.v[1] } else { edge.v[0] })
                })
                .collect();
            if attached.is_empty() {
                break;
            }
            vertices.extend(attached);
        }
        self.transform_vertices(&vertices, |p| {
            let delta = p - curve.centre;
            let perpendicular = normal * delta.dot(normal);
            curve.centre + perpendicular + (delta - perpendicular) * factor
        })
    }

    /// Resamples an intact radial curve in planar geometry. Faces retain their IDs
    /// and attributes. Arc endpoints can stay joined to other edges.
    pub fn set_curve_segments(&mut self, id: CurveId, segments: u32) -> Result<Vec<EdgeId>, MeshError> {
        let mut updated = self.clone();
        let edges = updated.resample_curve(id, segments)?;
        *self = updated;
        Ok(edges)
    }

    fn resample_curve(&mut self, id: CurveId, segments: u32) -> Result<Vec<EdgeId>, MeshError> {
        let curve = *self.curves.get(&id).ok_or(MeshError::Missing(id.0))?;
        let closed = matches!(curve.kind, CurveKind::Circle | CurveKind::Polygon);
        if curve.kind == CurveKind::Polyline {
            return Err(MeshError::Unsupported("a deformed curve has no radial segmentation".into()));
        }
        if segments < if closed { 3 } else { 2 } || segments > 4096 {
            return Err(MeshError::Invalid("segments must be 3-4096 for a closed curve, 2-4096 for an arc".into()));
        }
        let edges = self.curve_edges(id);
        if segments == curve.segments {
            return Ok(edges);
        }
        if edges.len() != curve.segments as usize {
            return Err(MeshError::Unsupported("split curves cannot be resampled".into()));
        }
        let members: BTreeSet<_> = edges.iter().flat_map(|e| self.edges[e].v).collect();
        let normal = curve.normal.normalize();
        let u = curve.start.normalize();
        let v = normal.cross(u);
        let point = |t: f64| {
            let angle = curve.sweep.to_radians() * t;
            curve.centre + (u * angle.cos() + v * angle.sin()) * curve.radius
        };
        let first = self.find_vertex(point(0.0)).ok_or_else(|| MeshError::Invalid("curve start is missing".into()))?;
        let last = if closed {
            first
        } else {
            self.find_vertex(point(1.0)).ok_or_else(|| MeshError::Invalid("curve end is missing".into()))?
        };
        let removed: BTreeSet<_> =
            members.iter().copied().filter(|vertex| closed || (*vertex != first && *vertex != last)).collect();
        if self
            .edges
            .values()
            .any(|edge| edge.curve != Some(id) && edge.v.iter().any(|vertex| removed.contains(vertex)))
        {
            return Err(MeshError::Unsupported(
                "segments cannot change where other edges join the curve; set them before Push/Pull or Follow Me"
                    .into(),
            ));
        }
        let touched: Vec<_> = self
            .faces
            .iter()
            .filter_map(|(face_id, face)| {
                face.loops().any(|vertices| vertices.iter().any(|v| members.contains(v))).then_some(*face_id)
            })
            .collect();
        for face_id in &touched {
            if self.face_plane(*face_id).is_none_or(|plane| plane.normal.dot(normal).abs() < 1.0 - 1e-6) {
                return Err(MeshError::Unsupported("segments can only change in planar geometry".into()));
            }
            for vertices in self.faces[face_id].loops() {
                let count = vertices.iter().filter(|vertex| removed.contains(vertex)).count();
                if count > 0
                    && (if closed {
                        count != vertices.len() || count != members.len()
                    } else {
                        !vertices.contains(&first) || !vertices.contains(&last) || count != removed.len()
                    })
                {
                    return Err(MeshError::Unsupported(
                        "partially attached curve boundaries cannot be resampled".into(),
                    ));
                }
            }
        }
        let count = if closed { segments } else { segments + 1 };
        let positions: Vec<_> = (0..count).map(|i| point(i as f64 / segments as f64)).collect();
        for (index, a) in positions.iter().enumerate().take(segments as usize) {
            let b = positions[(index + 1) % positions.len()];
            for (edge_id, edge) in &self.edges {
                if edges.contains(edge_id) {
                    continue;
                }
                let (p, q) = (self.pos(edge.v[0]), self.pos(edge.v[1]));
                if let Some((t, _)) = rysmo_math::segment_intersection(*a, b, p, q) {
                    let allowed = !closed
                        && ((index == 0 && t < EPS && edge.v.contains(&first))
                            || (index + 1 == segments as usize && t > 1.0 - EPS && edge.v.contains(&last)));
                    if !allowed {
                        return Err(MeshError::Unsupported("new segments intersect other geometry".into()));
                    }
                }
            }
        }
        let properties: Vec<_> = edges
            .iter()
            .map(|edge| {
                let flags = self.edges[edge].clone();
                ((self.pos(flags.v[0]) + self.pos(flags.v[1])) * 0.5, flags)
            })
            .collect();
        let vertices: Vec<_> = positions.iter().map(|p| self.add_vertex(*p)).collect();
        // Update every boundary, including reversed hole loops.
        for face_id in touched {
            let mut face = self.faces[&face_id].clone();
            for boundary in std::iter::once(&mut face.outer).chain(face.holes.iter_mut()) {
                if !boundary.iter().any(|v| removed.contains(v)) {
                    continue;
                }
                if closed {
                    let forward =
                        rysmo_math::Plane::from_polygon(&self.loop_points(boundary)).unwrap().normal.dot(normal) > 0.0;
                    *boundary = if forward { vertices.clone() } else { vertices.iter().rev().copied().collect() };
                } else {
                    let original = boundary.clone();
                    let mut replacement = Vec::new();
                    for (i, vertex) in original.iter().enumerate() {
                        if removed.contains(vertex) {
                            continue;
                        }
                        replacement.push(*vertex);
                        if removed.contains(&original[(i + 1) % original.len()]) {
                            if *vertex == first {
                                replacement.extend(vertices[1..vertices.len() - 1].iter().copied());
                            } else if *vertex == last {
                                replacement.extend(vertices[1..vertices.len() - 1].iter().rev().copied());
                            }
                        }
                    }
                    *boundary = replacement;
                }
            }
            self.faces.insert(face_id, face);
        }
        for edge in edges {
            self.remove_edge_raw(edge);
        }
        let mut result = Vec::new();
        for i in 0..segments as usize {
            let edge = self.raw_edge(vertices[i], vertices[(i + 1) % vertices.len()]);
            let midpoint = point((i as f64 + 0.5) / segments as f64);
            let flags = &properties
                .iter()
                .min_by(|a, b| a.0.distance_squared(midpoint).total_cmp(&b.0.distance_squared(midpoint)))
                .unwrap()
                .1;
            let e = self.edges.get_mut(&edge).unwrap();
            e.curve = Some(id);
            e.soft = flags.soft;
            e.smooth = flags.smooth;
            e.hidden = flags.hidden;
            e.material = flags.material;
            result.push(edge);
        }
        self.curves.get_mut(&id).unwrap().segments = segments;
        self.remove_orphan_vertices();
        self.validate()?;
        Ok(result)
    }

    pub(crate) fn map_touched_curves(&mut self, vertices: &BTreeSet<VertexId>, f: &impl Fn(DVec3) -> DVec3) {
        let curves: BTreeSet<_> =
            vertices.iter().flat_map(|v| self.edges_of_vertex(*v)).filter_map(|e| self.edges[&e].curve).collect();
        let view = self.view();
        for id in curves {
            let members: BTreeSet<_> = view.curve_edges(self, id).iter().flat_map(|e| self.edges[e].v).collect();
            if members.is_disjoint(vertices) {
                continue;
            }
            let curve = self.curves[&id];
            self.curves.insert(
                id,
                if members.is_subset(vertices) {
                    curve.mapped(f)
                } else {
                    Curve { kind: CurveKind::Polyline, ..curve }
                },
            );
        }
    }
    pub(crate) fn edges_on_segments(&self, lines: &[(DVec3, DVec3)]) -> Vec<EdgeId> {
        self.edges
            .keys()
            .copied()
            .filter(|id| {
                let (p, q) = self.edge_points(*id);
                lines.iter().any(|(a, b)| on_segment(p, *a, *b) && on_segment(q, *a, *b))
            })
            .collect()
    }
    pub(crate) fn prune_curves(&mut self) {
        let mut counts: BTreeMap<_, u32> = BTreeMap::new();
        for curve in self.edges.values().filter_map(|e| e.curve) {
            *counts.entry(curve).or_default() += 1;
        }
        self.curves.retain(|id, _| counts.contains_key(id));
        for (id, count) in counts {
            if let Some(curve) = self.curves.get(&id)
                && curve.kind == CurveKind::Polyline
                && curve.segments != count
            {
                self.curves.get_mut(&id).expect("just found").segments = count;
            }
        }
    }
}

/// Whether `p` lies on the segment `a..b`, the way a curve claims its edges.
pub(crate) fn on_segment(p: DVec3, a: DVec3, b: DVec3) -> bool {
    let d = b - a;
    if d.length_squared() < EPS * EPS {
        return false;
    }
    let t = (p - a).dot(d) / d.length_squared();
    (-EPS..=1.0 + EPS).contains(&t) && p.distance(a + d * t) < EPS * 2.0
}
