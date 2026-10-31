//! The inference engine: what point the cursor means.
//!
//! The drawing tools never take the raw cursor position. They ask what the cursor
//! is near: an endpoint (green), the midpoint of an edge (cyan), a point on an edge (red),
//! a face (blue), a line along a model axis from the last point (red, green or blue), or
//! the intersection of two such lines. This crate answers that question from the mesh,
//! the camera and the cursor, with no UI attached, so it can be tested directly.
//!
//! A tool can lock the answer to a line or a plane (Shift, or an arrow key for an
//! axis); later queries then slide along that constraint and only borrow other
//! inferences by projecting them onto it.

#![forbid(unsafe_code)]

use rysmo_math::{
    Axis, DMat4, DVec2, DVec3, EPS, Plane, Ray, View, closest_line_ray, closest_on_segment, distance_point_segment_2d,
};
use rysmo_mesh::spatial::{Aabb, Bvh};
use rysmo_mesh::{EdgeId, Element, FaceId, GuideId, InstanceId, Mesh};
use std::sync::Arc;

/// Screen distance within which points snap, in pixels.
pub const POINT_PX: f64 = 10.0;
/// Screen distance within which edges and axis lines snap, in pixels.
pub const LINE_PX: f64 = 7.0;

/// What kind of point was found. Each kind has a colour and tooltip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Endpoint,
    Midpoint,
    Centre,
    OnEdge,
    Parallel,
    Perpendicular,
    Tangent,
    OnFace,
    Origin,
    /// On a line along an axis from the reference point.
    OnAxis(Axis),
    /// On a line along an axis from a point the cursor rested on earlier.
    FromPoint(Axis),
    /// Where two axis lines cross.
    Intersection,
    /// On the ground plane (Z = 0), with nothing else nearby.
    Ground,
    /// A guide point made with the Tape Measure.
    GuidePoint,
    /// On a guide line made with the Tape Measure.
    OnGuide,
    /// Nothing to snap to: a point in space at the depth of the reference.
    Free,
}

impl Kind {
    /// The tooltip the editor shows.
    pub fn tooltip(self) -> &'static str {
        match self {
            Kind::Endpoint => "Endpoint",
            Kind::Midpoint => "Midpoint",
            Kind::Centre => "Center",
            Kind::OnEdge => "On Edge",
            Kind::Parallel => "Parallel",
            Kind::Perpendicular => "Perpendicular",
            Kind::Tangent => "Tangent",
            Kind::OnFace => "On Face",
            Kind::Origin => "Origin",
            Kind::OnAxis(Axis::X) => "On Red Axis",
            Kind::OnAxis(Axis::Y) => "On Green Axis",
            Kind::OnAxis(Axis::Z) => "On Blue Axis",
            Kind::FromPoint(_) => "From Point",
            Kind::Intersection => "Intersection",
            Kind::Ground => "On Ground",
            Kind::GuidePoint => "Guide Point",
            Kind::OnGuide => "On Line",
            Kind::Free => "",
        }
    }

    /// The marker colour, sRGB 0..1: green endpoints, cyan midpoints, red edges, blue
    /// faces, and the axis colours.
    pub fn color(self) -> [f32; 3] {
        match self {
            Kind::Endpoint => [0.0, 0.75, 0.0],
            Kind::Midpoint | Kind::Centre => [0.0, 0.8, 0.85],
            Kind::OnEdge | Kind::Intersection => [0.9, 0.0, 0.0],
            Kind::Parallel | Kind::Perpendicular | Kind::Tangent => [0.85, 0.1, 0.8],
            Kind::OnFace => [0.15, 0.3, 1.0],
            Kind::Origin => [0.9, 0.75, 0.0],
            Kind::OnAxis(a) | Kind::FromPoint(a) => axis_color(a),
            Kind::GuidePoint => [0.0, 0.0, 0.0],
            Kind::OnGuide => [0.9, 0.0, 0.0],
            Kind::Ground | Kind::Free => [0.3, 0.3, 0.3],
        }
    }
}

/// Red, green and blue for X, Y and Z.
pub fn axis_color(a: Axis) -> [f32; 3] {
    match a {
        Axis::X => [0.9, 0.0, 0.0],
        Axis::Y => [0.0, 0.7, 0.0],
        Axis::Z => [0.0, 0.0, 0.95],
    }
}

/// A constraint the tool holds while the user keeps Shift down or an axis key locked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lock {
    /// Slide along a line.
    Line { origin: DVec3, dir: DVec3, axis: Option<Axis> },
    /// Stay in a plane.
    Plane(Plane),
}

/// What the cursor means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inference {
    pub point: DVec3,
    pub kind: Kind,
    /// The edge the point is on or at the end of, if any.
    pub edge: Option<EdgeId>,
    pub curve: Option<rysmo_mesh::CurveId>,
    /// The face under the cursor (also set for points on edges in front of it).
    pub face: Option<FaceId>,
    /// The plane the point lies in, for tools that draw in a plane: the face's, or the
    /// ground's.
    pub plane: Option<Plane>,
    /// A dotted guide to draw, from a reference point to the inferred point.
    pub guide: Option<(DVec3, DVec3, Kind)>,
    /// The group or component the point is on, if it is on one (a child of the
    /// context being edited).
    pub instance: Option<InstanceId>,
    /// The construction guide the point is on, if any.
    pub construction: Option<GuideId>,
}

impl Inference {
    /// The constraint to hold if the user presses Shift now, relative to `from`.
    pub fn lock_for(&self, from: Option<DVec3>, mesh: &Mesh) -> Option<Lock> {
        match self.kind {
            Kind::OnAxis(a) => from.map(|o| Lock::Line {
                origin: o,
                dir: self.guide.map(|(a, b, _)| (b - a).normalize()).unwrap_or_else(|| a.dir()),
                axis: Some(a),
            }),
            Kind::FromPoint(a) => self.guide.map(|(o, _, _)| Lock::Line {
                origin: o,
                dir: self.guide.map(|(a, b, _)| (b - a).normalize()).unwrap_or_else(|| a.dir()),
                axis: Some(a),
            }),
            Kind::OnEdge => self.edge.filter(|e| mesh.edge(*e).is_some()).map(|e| {
                let (a, b) = mesh.edge_points(e);
                Lock::Line { origin: a, dir: (b - a).normalize(), axis: None }
            }),
            Kind::Parallel | Kind::Perpendicular | Kind::Tangent => self
                .guide
                .and_then(|(a, b, _)| (b - a).try_normalize().map(|dir| Lock::Line { origin: a, dir, axis: None })),
            Kind::OnFace => self.plane.map(Lock::Plane),
            _ => None,
        }
    }
}

/// Everything a query needs.
pub struct Query<'a> {
    pub show_hidden: bool,
    pub axes: rysmo_math::DrawingAxes,
    pub mesh: &'a Mesh,
    pub view: &'a View,
    pub cursor: DVec2,
    /// The last point of the current operation (the start of a line), for axis
    /// inferences.
    pub from: Option<DVec3>,
    pub lock: Option<Lock>,
    /// Points the cursor rested on recently, for "from point" inferences.
    pub hints: &'a [DVec3],
    /// Pixel tolerance multiplier: larger on touch screens.
    pub scale: f64,
    /// Other visible geometry (groups, components, the faded model around the context
    /// being edited), with transforms into the context's coordinates.
    pub others: &'a [Other<'a>],
    /// Trees over the meshes, so a query does not have to visit all of them.
    pub accel: Option<&'a Accel>,
    /// Construction guides of the context.
    pub guides: &'a [(GuideId, SnapGuide)],
    pub clips: Vec<Plane>,
}

/// Trees over one mesh's edges and faces.
struct Trees {
    edges: Bvh<EdgeId>,
    faces: Bvh<FaceId>,
    /// The mesh they were built from.
    mesh: Mesh,
}

impl Trees {
    fn over(
        mesh: &Mesh,
        edges: impl Iterator<Item = EdgeId>,
        faces: impl Iterator<Item = FaceId>,
    ) -> (Bvh<EdgeId>, Bvh<FaceId>) {
        let margin = EPS * 4.;
        let edges = edges.filter_map(|id| Some((Aabb::of(mesh.edge(id)?.v.map(|v| mesh.pos(v))).expanded(margin), id)));
        let edges = Bvh::new(edges.collect());
        let faces = faces
            .filter_map(|id| Some((Aabb::of(mesh.face(id)?.outer.iter().map(|v| mesh.pos(*v))).expanded(margin), id)));
        (edges, Bvh::new(faces.collect()))
    }
}

/// Lookups in one mesh: full trees of an earlier state, shared between updates, and
/// small trees of what has changed since. An element that moved or went is still in the
/// full trees at its old place, which only costs callers a look at the mesh.
pub struct MeshIndex {
    base: Arc<Trees>,
    edges: Bvh<EdgeId>,
    faces: Bvh<FaceId>,
    /// Everything a query can reach, curve centres included.
    bounds: Aabb,
}

impl MeshIndex {
    pub fn new(mesh: &Mesh) -> Self {
        let (edges, faces) = Trees::over(mesh, mesh.edges().map(|(id, _)| id), mesh.faces().map(|(id, _)| id));
        let base = Arc::new(Trees { edges, faces, mesh: mesh.clone() });
        MeshIndex { base, edges: Bvh::default(), faces: Bvh::default(), bounds: Self::reach(mesh) }
    }

    /// The index of `mesh`, a later state of the one `self` was made for.
    pub fn update(&self, mesh: &Mesh) -> Self {
        let (mut faces, vertices) = mesh.changed_since(&self.base.mesh);
        let mut edges: Vec<_> = mesh.changed_edges_since(&self.base.mesh).into_iter().collect();
        if (edges.len() + faces.len()) * REBUILD_SHARE > self.base.edges.len() + self.base.faces.len() {
            return Self::new(mesh);
        }
        edges.extend(vertices.iter().flat_map(|v| mesh.edges_of_vertex(*v)));
        if !vertices.is_empty() {
            let vertices: std::collections::HashSet<_> = vertices.into_iter().collect();
            let touches = |(id, face): (FaceId, &rysmo_mesh::Face)| {
                face.loops().flatten().any(|v| vertices.contains(v)).then_some(id)
            };
            faces.extend(mesh.faces().filter_map(touches));
        }
        let (edges, faces) = Trees::over(mesh, edges.into_iter(), faces.into_iter());
        MeshIndex { base: self.base.clone(), edges, faces, bounds: self.bounds.union(Self::reach(mesh)) }
    }

    fn reach(mesh: &Mesh) -> Aabb {
        let margin = EPS * 4.;
        let lines = mesh.edges().map(|(_, e)| Aabb::of(e.v.map(|v| mesh.pos(v))));
        let centres = mesh.curves().map(|(_, curve)| Aabb::at(curve.centre));
        lines.chain(centres).fold(Aabb::EMPTY, Aabb::union).expanded(margin)
    }

    fn visit_edges(&self, mut enter: impl FnMut(&Aabb) -> bool, found: &mut Vec<EdgeId>) {
        self.base.edges.visit(&mut enter, |id| found.push(*id));
        self.edges.visit(&mut enter, |id| found.push(*id));
        found.sort_unstable();
        found.dedup();
    }

    fn visit_faces(&self, mut enter: impl FnMut(&Aabb) -> bool, found: &mut Vec<FaceId>) {
        self.base.faces.visit(&mut enter, |id| found.push(*id));
        self.faces.visit(&mut enter, |id| found.push(*id));
        found.sort_unstable();
        found.dedup();
    }

    pub fn bounds(&self) -> Aabb {
        self.bounds
    }
}

/// `MeshIndex::update` starts over once changes reach one part in this many.
const REBUILD_SHARE: usize = 8;

/// Indexes of a query's meshes, and a tree of where the other meshes sit. `others` and
/// `from_local` run parallel to `Query::others`.
pub struct Accel {
    active: Arc<MeshIndex>,
    others: Vec<Arc<MeshIndex>>,
    from_local: Vec<DMat4>,
    placed: Bvh<usize>,
}

impl Accel {
    /// `others` pairs each mesh's index with the transform into the context.
    pub fn new(active: Arc<MeshIndex>, others: Vec<(Arc<MeshIndex>, DMat4)>) -> Self {
        let placed = others
            .iter()
            .enumerate()
            .filter(|(_, (index, _))| index.bounds != Aabb::EMPTY)
            .map(|(i, (index, t))| (Aabb::of(index.bounds.corners().map(|p| t.transform_point3(p))), i));
        let placed = Bvh::new(placed.collect());
        let from_local = others.iter().map(|(_, t)| t.inverse()).collect();
        Accel { active, others: others.into_iter().map(|(index, _)| index).collect(), from_local, placed }
    }
}

/// Geometry outside the mesh being edited that points can still snap to.
#[derive(Clone)]
pub struct Other<'a> {
    pub clips: Vec<Plane>,
    pub mesh: std::borrow::Cow<'a, Mesh>,
    /// The other mesh's coordinates to the context's.
    pub to_local: DMat4,
    /// The child of the context it belongs to (what a click selects), or `None` for
    /// geometry outside the context.
    pub instance: Option<InstanceId>,
}

/// A construction guide as the inference engine sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapGuide {
    Line { origin: DVec3, dir: DVec3 },
    Point(DVec3),
    Segment { start: DVec3, point: DVec3 },
}

fn guide_endpoint(guide: SnapGuide) -> Option<DVec3> {
    match guide {
        SnapGuide::Point(p) | SnapGuide::Segment { point: p, .. } => Some(p),
        _ => None,
    }
}

/// One mesh to search, with its transform and owner.
struct Src<'a> {
    show_hidden: bool,
    mesh: &'a Mesh,
    to_local: DMat4,
    from_local: DMat4,
    instance: Option<InstanceId>,
    active: bool,
    clips: &'a [Plane],
    index: Option<&'a MeshIndex>,
}

impl Src<'_> {
    fn p(&self, p: DVec3) -> DVec3 {
        if self.active { p } else { self.to_local.transform_point3(p) }
    }

    fn visible(&self, point: DVec3) -> bool {
        self.clips.iter().all(|plane| plane.distance(point) <= 1e-9)
    }
    fn clipped_edge(&self, mut line: [DVec3; 2]) -> Option<[DVec3; 2]> {
        for plane in self.clips {
            let [a, b] = line.map(|p| plane.distance(p));
            if a > 1e-9 && b > 1e-9 {
                return None;
            }
            if (a > 1e-9) != (b > 1e-9) {
                let point = line[0].lerp(line[1], a / (a - b));
                line[if a > 1e-9 { 0 } else { 1 }] = point;
            }
        }
        Some(line)
    }

    /// The edges whose screen box comes within `tol` of the cursor, in id order.
    fn edges_near_cursor(&self, q: &Query, tol: f64) -> Vec<EdgeId> {
        let Some(index) = self.index else { return self.mesh.edges().map(|(id, _)| id).collect() };
        let mut found = Vec::new();
        index.visit_edges(|b| q.reaches(b.corners().map(|p| self.p(p)), tol), &mut found);
        found
    }

    /// The edges through any of `points`, given in the context's coordinates, in id order.
    fn edges_at(&self, points: &[DVec3]) -> Vec<EdgeId> {
        let Some(index) = self.index else { return self.mesh.edges().map(|(id, _)| id).collect() };
        let mut found = Vec::new();
        for point in points {
            let area = Aabb::at(self.from_local.transform_point3(*point));
            index.visit_edges(|b| b.overlaps(&area), &mut found);
        }
        found
    }

    /// The faces whose box `ray` enters, in id order.
    fn faces_on_ray(&self, ray: &Ray) -> Vec<FaceId> {
        let Some(index) = self.index else { return self.mesh.faces().map(|(id, _)| id).collect() };
        let mut found = Vec::new();
        index.visit_faces(|b| b.entry(ray, f64::INFINITY).is_some(), &mut found);
        found.retain(|id| self.mesh.face(*id).is_some());
        found
    }

    /// The nearest face the ray hits, with the distance along the context ray.
    fn pick_face(&self, ray: &Ray) -> Option<(FaceId, f64)> {
        if !self.clips.is_empty() {
            let local =
                Ray::new(self.from_local.transform_point3(ray.origin), self.from_local.transform_vector3(ray.dir));
            return self
                .faces_on_ray(&local)
                .into_iter()
                .filter(|id| self.show_hidden || self.mesh.face(*id).is_some_and(|f| !f.hidden))
                .filter_map(|id| {
                    let plane = self.mesh.face_plane(id)?;
                    let distance = plane.intersect_ray(&local)?;
                    let point = local.at(distance);
                    if !self.mesh.face_contains(id, point) {
                        return None;
                    }
                    let point = self.p(point);
                    self.visible(point).then_some((id, (point - ray.origin).dot(ray.dir)))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
        }
        if self.active {
            return self.mesh.pick_face_among(self.faces_on_ray(ray), ray, self.show_hidden);
        }
        let o = self.from_local.transform_point3(ray.origin);
        let d = self.from_local.transform_vector3(ray.dir);
        if d.length() < 1e-12 {
            return None;
        }
        let local = Ray::new(o, d);
        let (f, t) = self.mesh.pick_face_among(self.faces_on_ray(&local), &local, self.show_hidden)?;
        let hit = self.to_local.transform_point3(Ray::new(o, d).at(t));
        Some((f, (hit - ray.origin).dot(ray.dir)))
    }
}

impl<'a> Query<'a> {
    /// A query with nothing but the mesh, the view and the cursor.
    pub fn simple(mesh: &'a Mesh, view: &'a View, cursor: DVec2, scale: f64) -> Self {
        Query {
            show_hidden: false,
            clips: Vec::new(),
            axes: Default::default(),
            mesh,
            view,
            cursor,
            from: None,
            lock: None,
            hints: &[],
            scale,
            others: &[],
            accel: None,
            guides: &[],
        }
    }

    fn source(&self, i: usize) -> Src<'_> {
        let o = &self.others[i];
        Src {
            show_hidden: self.show_hidden,
            mesh: &o.mesh,
            to_local: o.to_local,
            from_local: self.accel.map_or_else(|| o.to_local.inverse(), |a| a.from_local[i]),
            instance: o.instance,
            active: false,
            clips: &o.clips,
            index: self.accel.map(|a| a.others[i].as_ref()),
        }
    }

    /// The mesh being edited, and the others where `enter` accepts their box. Without
    /// trees that is every mesh.
    fn sources_where(&self, enter: impl FnMut(&Aabb) -> bool) -> Vec<Src<'_>> {
        let mut out = vec![Src {
            show_hidden: self.show_hidden,
            mesh: self.mesh,
            to_local: DMat4::IDENTITY,
            from_local: DMat4::IDENTITY,
            instance: None,
            active: true,
            clips: &self.clips,
            index: self.accel.map(|a| a.active.as_ref()),
        }];
        let Some(accel) = self.accel else {
            out.extend((0..self.others.len()).map(|i| self.source(i)));
            return out;
        };
        let mut found = Vec::new();
        accel.placed.visit(enter, |i| found.push(*i));
        found.sort_unstable();
        out.extend(found.into_iter().map(|i| self.source(i)));
        out
    }

    fn sources_near_cursor(&self, tol: f64) -> Vec<Src<'_>> {
        self.sources_where(|b| self.reaches(b.corners(), tol))
    }

    fn sources_on_ray(&self, ray: &Ray, limit: f64) -> Vec<Src<'_>> {
        self.sources_where(|b| b.entry(ray, limit).is_some())
    }

    fn sources_at(&self, points: &[DVec3]) -> Vec<Src<'_>> {
        let area = Aabb::of(points.iter().copied());
        self.sources_where(|b| b.overlaps(&area))
    }

    /// Whether a box with these corners can come within `tol` of the cursor on screen.
    fn reaches(&self, corners: [DVec3; 8], tol: f64) -> bool {
        let (mut lo, mut hi) = (DVec2::INFINITY, DVec2::NEG_INFINITY);
        for corner in corners {
            // A corner behind the eye has no place on screen to judge the box by.
            let Some(s) = self.screen(corner) else { return true };
            lo = lo.min(s);
            hi = hi.max(s);
        }
        (self.cursor - lo).min_element() >= -tol && (hi - self.cursor).min_element() >= -tol
    }

    fn ray(&self) -> Ray {
        self.view.ray(self.cursor)
    }

    fn screen(&self, p: DVec3) -> Option<DVec2> {
        self.view.project(p)
    }

    /// Whether a point is hidden behind a face, seen from the camera.
    fn occluded(&self, p: DVec3) -> bool {
        let to = p - self.view.eye;
        let dist = to.length();
        if dist < EPS {
            return false;
        }
        let ray = Ray::new(self.view.eye, to);
        self.sources_on_ray(&ray, dist)
            .iter()
            .any(|s| s.pick_face(&ray).is_some_and(|(_, t)| t > 0.0 && t < dist - dist * 1e-6 - EPS * 10.0))
    }

    /// The point on an infinite line closest to the cursor ray, and how far from the
    /// cursor the line is on screen.
    fn on_line(&self, origin: DVec3, dir: DVec3) -> Option<(DVec3, f64)> {
        let (s, _) = closest_line_ray(origin, dir, &self.ray())?;
        let p = origin + dir * s;
        let sp = self.screen(p)?;
        Some((p, self.cursor.distance(sp)))
    }
}

/// Finds what the cursor means.
pub fn infer(q: &Query) -> Inference {
    match q.lock {
        Some(Lock::Line { origin, dir, axis }) => locked_line(q, origin, dir, axis),
        Some(Lock::Plane(plane)) => locked_plane(q, plane),
        None => free(q),
    }
}

fn base(point: DVec3, kind: Kind) -> Inference {
    Inference {
        point,
        kind,
        curve: None,
        edge: None,
        face: None,
        plane: None,
        guide: None,
        instance: None,
        construction: None,
    }
}

/// Snapped points (endpoints, midpoints, the origin) near the cursor, nearest first.
fn point_snap(q: &Query) -> Option<Inference> {
    let tol = POINT_PX * q.scale;
    let mut best: Option<(f64, Inference)> = None;
    let consider = |d: f64, inf: Inference, best: &mut Option<(f64, Inference)>| {
        if d <= tol && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
            *best = Some((d, inf));
        }
    };
    for src in q.sources_near_cursor(tol) {
        let near: Vec<_> = src
            .edges_near_cursor(q, tol)
            .into_iter()
            .filter_map(|id| Some((id, src.mesh.edge(id)?)))
            .filter(|(_, e)| !e.procedural && (q.show_hidden || e.visible()))
            .collect();
        let visible_vertices: std::collections::BTreeSet<_> = near.iter().flat_map(|(_, e)| e.v).collect();
        for id in visible_vertices {
            let p = src.p(src.mesh.pos(id));
            if !src.visible(p) {
                continue;
            }
            if let Some(s) = q.screen(p) {
                let d = q.cursor.distance(s);
                if d <= tol {
                    let mut inf = base(p, Kind::Endpoint);
                    inf.instance = src.instance;
                    consider(d, inf, &mut best);
                }
            }
        }
        for (id, curve) in src.mesh.curves() {
            // Checked first: telling a curve's edges apart means walking all the edges.
            if q.screen(src.p(curve.centre)).is_none_or(|s| q.cursor.distance(s) + 0.75 > tol) {
                continue;
            }
            if curve.kind == rysmo_mesh::CurveKind::Polyline
                || !src
                    .mesh
                    .curve_edges(id)
                    .iter()
                    .any(|id| src.mesh.edge(*id).is_some_and(|e| !e.procedural && (q.show_hidden || e.visible())))
            {
                continue;
            }
            let point = src.p(curve.centre);
            if !src.visible(point) {
                continue;
            }
            if let Some(screen) = q.screen(point) {
                let mut inf = base(point, Kind::Centre);
                inf.curve = (src.active || src.instance.is_some()).then_some(id);
                inf.instance = src.instance;
                let normal = if src.active {
                    curve.normal
                } else {
                    src.to_local.inverse().transpose().transform_vector3(curve.normal).normalize()
                };
                inf.plane = Some(Plane::new(normal, point));
                consider(q.cursor.distance(screen) + 0.75, inf, &mut best);
            }
        }
        for (id, e) in near {
            let m = src.p((src.mesh.pos(e.v[0]) + src.mesh.pos(e.v[1])) * 0.5);
            if !src.visible(m) {
                continue;
            }
            if let Some(s) = q.screen(m) {
                // Endpoints win ties with midpoints.
                let d = q.cursor.distance(s) + 0.5;
                if d <= tol {
                    let mut inf = base(m, Kind::Midpoint);
                    inf.edge = src.active.then_some(id);
                    inf.instance = src.instance;
                    consider(d, inf, &mut best);
                }
            }
        }
    }
    for (id, g) in q.guides {
        if let Some(p) = guide_endpoint(*g)
            && let Some(s) = q.screen(p)
        {
            let mut inf = base(p, Kind::GuidePoint);
            inf.construction = Some(*id);
            consider(q.cursor.distance(s), inf, &mut best);
        }
    }
    if let Some(s) = q.screen(q.axes.origin) {
        consider(q.cursor.distance(s) + 1.0, base(q.axes.origin, Kind::Origin), &mut best);
    }
    let (_, mut inf) = best?;
    if q.occluded(inf.point) {
        return None;
    }
    if inf.edge.is_none() && inf.kind == Kind::Endpoint && inf.instance.is_none() {
        inf.edge = q.mesh.find_vertex(inf.point).and_then(|v| q.mesh.edges_of_vertex(v).first().copied());
    }
    Some(inf)
}

fn edge_snap(q: &Query) -> Option<Inference> {
    let tol = LINE_PX * q.scale;
    let ray = q.ray();
    let mut best: Option<(f64, Inference)> = None;
    for src in q.sources_near_cursor(tol) {
        for id in src.edges_near_cursor(q, tol) {
            let Some(e) = src.mesh.edge(id) else { continue };
            if e.procedural || (!q.show_hidden && !e.visible()) {
                continue;
            }
            let Some([a, b]) = src.clipped_edge([src.p(src.mesh.pos(e.v[0])), src.p(src.mesh.pos(e.v[1]))]) else {
                continue;
            };
            let Some((sa, sb)) = q.view.project_segment(a, b) else { continue };
            let d = distance_point_segment_2d(q.cursor, sa, sb);
            if d > tol {
                continue;
            }
            let dir = b - a;
            let len = dir.length();
            if len < EPS {
                continue;
            }
            let Some((s, _)) = closest_line_ray(a, dir / len, &ray) else { continue };
            let p = a + dir / len * s.clamp(0.0, len);
            if q.occluded(p) {
                continue;
            }
            if best.as_ref().is_some_and(|(distance, existing)| {
                if (d - distance).abs() <= 1e-6 {
                    (p - ray.origin).dot(ray.dir) >= (existing.point - ray.origin).dot(ray.dir)
                } else {
                    d >= *distance
                }
            }) {
                continue;
            }
            let mut inf = base(p, Kind::OnEdge);
            inf.edge = src.active.then_some(id);
            inf.instance = src.instance;
            best = Some((d, inf));
        }
    }
    for (id, guide) in q.guides {
        if let SnapGuide::Segment { start, point } = guide
            && let Some((a, b)) = q.view.project_segment(*start, *point)
        {
            let distance = distance_point_segment_2d(q.cursor, a, b);
            if distance > tol || best.as_ref().is_some_and(|(d, _)| distance >= *d) {
                continue;
            }
            let delta = *point - *start;
            let Some(dir) = delta.try_normalize() else { continue };
            let Some((t, _)) = closest_line_ray(*start, dir, &ray) else { continue };
            let point = *start + dir * t.clamp(0., delta.length());
            if q.occluded(point) {
                continue;
            }
            let mut inf = base(point, Kind::OnGuide);
            inf.construction = Some(*id);
            best = Some((distance, inf));
        }
    }
    for (id, g) in q.guides {
        if let SnapGuide::Line { origin, dir } = g
            && let Some((p, d)) = q.on_line(*origin, *dir)
            && d <= tol
            && best.as_ref().is_none_or(|(bd, _)| d < *bd)
        {
            let mut inf = base(p, Kind::OnGuide);
            inf.construction = Some(*id);
            best = Some((d, inf));
        }
    }
    best.map(|(_, i)| i)
}

/// Axis lines through `origin` near the cursor: (point, axis, screen distance).
fn axis_snaps(q: &Query, origin: DVec3) -> Vec<(DVec3, Axis, f64)> {
    let tol = LINE_PX * q.scale;
    let mut out: Vec<(DVec3, Axis, f64)> = Axis::ALL
        .iter()
        .filter_map(|a| {
            let (p, d) = q.on_line(origin, q.axes.dir(*a))?;
            (d <= tol && p.distance(origin) > EPS).then_some((p, *a, d))
        })
        .collect();
    out.sort_by(|x, y| x.2.total_cmp(&y.2));
    out
}

fn surface(q: &Query) -> Inference {
    let ray = q.ray();
    let mut best: Option<(f64, Inference)> = None;
    for src in q.sources_on_ray(&ray, f64::INFINITY) {
        if let Some((f, t)) = src.pick_face(&ray)
            && t > 0.0
            && best.as_ref().is_none_or(|(bt, _)| t < *bt)
        {
            let mut inf = base(ray.at(t), Kind::OnFace);
            if src.active {
                inf.face = Some(f);
                inf.plane = q.mesh.face_plane(f);
            } else {
                inf.instance = src.instance;
                inf.plane = src.mesh.face_plane(f).and_then(|pl| {
                    let p0 = src.p(pl.normal * pl.d);
                    let n = src.from_local.transpose().transform_vector3(pl.normal);
                    (n.length() > 1e-12).then(|| Plane::new(n, p0))
                });
            }
            best = Some((t, inf));
        }
    }
    if let Some((_, inf)) = best {
        return inf;
    }
    let ground = q.axes.plane();
    if let Some(t) = ground.intersect_ray(&ray)
        && t < 1e5
    {
        let mut inf = base(ray.at(t), Kind::Ground);
        inf.plane = Some(ground);
        return inf;
    }
    // Looking at the sky: a point at the depth of the reference, or ahead.
    let depth = q.from.map(|f| (f - ray.origin).dot(ray.dir)).filter(|d| *d > 0.0).unwrap_or(10.0);
    base(ray.at(depth), Kind::Free)
}

fn free(q: &Query) -> Inference {
    let under = surface(q);
    let with_face = |mut i: Inference| {
        if i.face.is_none() && i.instance.is_none() {
            i.face = under.face;
        }
        i
    };
    if let Some(p) = point_snap(q) {
        return with_face(p);
    }
    // Axis lines from the reference point and from resting points; where two cross,
    // that is the strongest inference after points.
    let from_axes = q.from.map(|o| axis_snaps(q, o)).unwrap_or_default();
    let mut hint_axes: Vec<(DVec3, Axis, f64, DVec3)> = Vec::new();
    for h in q.hints {
        if q.from.is_some_and(|f| f.distance(*h) < EPS) {
            continue;
        }
        for (p, a, d) in axis_snaps(q, *h) {
            hint_axes.push((p, a, d, *h));
        }
    }
    hint_axes.sort_by(|x, y| x.2.total_cmp(&y.2));
    if let (Some(o), Some(&(_, fa, _))) = (q.from, from_axes.first()) {
        for &(_, ha, _, h) in &hint_axes {
            if ha == fa {
                continue;
            }
            if let Some(x) = line_intersection(o, q.axes.dir(fa), h, q.axes.dir(ha))
                && let Some(s) = q.screen(x)
                && q.cursor.distance(s) <= POINT_PX * q.scale * 1.5
            {
                let mut inf = base(x, Kind::Intersection);
                inf.guide = Some((h, x, Kind::FromPoint(ha)));
                return with_face(inf);
            }
        }
    }
    if let Some(e) = edge_snap(q) {
        return with_face(e);
    }
    if let Some(inf) = directional_snap(q, under.plane) {
        return with_face(inf);
    }
    if let (Some(o), Some(&(p, a, _))) = (q.from, from_axes.first()) {
        let mut inf = base(p, Kind::OnAxis(a));
        inf.guide = Some((o, p, Kind::OnAxis(a)));
        return with_face(inf);
    }
    if let Some(&(p, a, _, h)) = hint_axes.first() {
        let mut inf = base(p, Kind::FromPoint(a));
        inf.guide = Some((h, p, Kind::FromPoint(a)));
        return with_face(inf);
    }
    under
}

/// Acquire edge directions at a connected start or an explicitly visited endpoint.
/// Point/edge hits retain priority; only a narrow screen corridor attracts a direction.
fn directional_snap(q: &Query, plane: Option<Plane>) -> Option<Inference> {
    let from = q.from?;
    let plane = plane.unwrap_or_else(|| q.axes.plane());
    let mut best: Option<(f64, Inference)> = None;
    let mut offer = |dir: DVec3, kind: Kind| {
        let Some(dir) = dir.try_normalize() else { return };
        let Some((p, distance)) = q.on_line(from, dir) else { return };
        if distance > LINE_PX * q.scale || p.distance(from) < EPS || q.occluded(p) {
            return;
        }
        // Avoid a large angular attraction close to the starting point.
        let Some(a) = q.screen(from) else { return };
        if a.distance(q.cursor) < POINT_PX * q.scale * 2. {
            return;
        }
        if best.as_ref().is_none_or(|(d, _)| distance < *d) {
            let mut inf = base(p, kind);
            inf.guide = Some((from, p, kind));
            inf.plane = Some(plane);
            best = Some((distance, inf));
        }
    };
    let anchors: Vec<_> = std::iter::once(from).chain(q.hints.iter().copied()).collect();
    for src in q.sources_at(&anchors) {
        for id in src.edges_at(&anchors) {
            let Some(edge) = src.mesh.edge(id) else { continue };
            if edge.procedural || (!q.show_hidden && !edge.visible()) {
                continue;
            }
            let Some([a, b]) = src.clipped_edge([src.p(src.mesh.pos(edge.v[0])), src.p(src.mesh.pos(edge.v[1]))])
            else {
                continue;
            };
            let acquired = closest_on_segment(a, b, from).0.distance(from) < EPS
                || q.hints.iter().any(|h| closest_on_segment(a, b, *h).0.distance(*h) < EPS);
            if !acquired {
                continue;
            }
            // Analytic arcs supply their tangent rather than their tessellation chords.
            if edge.curve.is_some() {
                continue;
            }
            offer(b - a, Kind::Parallel);
            if (b - a).dot(plane.normal).abs() < EPS && plane.distance(from).abs() < EPS {
                offer(plane.normal.cross(b - a), Kind::Perpendicular);
            }
        }
        for (curve_id, curve) in src.mesh.curves() {
            let curve = curve.mapped(&|p| src.p(p));
            if !matches!(curve.kind, rysmo_mesh::CurveKind::Circle | rysmo_mesh::CurveKind::Arc) {
                continue;
            }
            let radial = from - curve.centre;
            if radial.dot(curve.normal).abs() > EPS || (radial.length() - curve.radius).abs() > EPS {
                continue;
            }
            // Only a real visible curve endpoint/segment can acquire this direction.
            if !src.visible(from) || q.occluded(from) {
                continue;
            }
            let local = src.from_local.transform_point3(from);
            if !src.mesh.edges().any(|(_, e)| {
                e.curve == Some(curve_id)
                    && !e.procedural
                    && (q.show_hidden || e.visible())
                    && closest_on_segment(src.mesh.pos(e.v[0]), src.mesh.pos(e.v[1]), local).0.distance(local) < EPS
            }) {
                continue;
            }
            offer(curve.normal.cross(radial), Kind::Tangent);
        }
    }
    best.map(|(_, inf)| inf)
}

/// Where two lines cross, if they do (within the tolerance).
fn line_intersection(a: DVec3, da: DVec3, b: DVec3, db: DVec3) -> Option<DVec3> {
    let n = da.cross(db);
    if n.length_squared() < 1e-12 {
        return None;
    }
    let w = b - a;
    let s = w.cross(db).dot(n) / n.length_squared();
    let t = w.cross(da).dot(n) / n.length_squared();
    let pa = a + da * s;
    let pb = b + db * t;
    (pa.distance(pb) < EPS).then_some(pa)
}

fn locked_line(q: &Query, origin: DVec3, dir: DVec3, axis: Option<Axis>) -> Inference {
    // Along an axis the line takes the axis colour; along anything else (an edge held
    // with Shift, a face normal in Push/Pull) the point is unremarkable.
    let kind = axis.map(Kind::OnAxis).unwrap_or(Kind::Free);
    // A snapped point projects onto the line, so a length can be matched to it.
    if let Some(p) = point_snap(q) {
        let (c, _) = closest_on_line(origin, dir, p.point);
        let mut inf = base(c, kind);
        inf.guide = Some((p.point, c, p.kind));
        return inf;
    }
    let p = match closest_line_ray(origin, dir, &q.ray()) {
        Some((s, _)) => origin + dir * s,
        None => origin,
    };
    let mut inf = base(p, kind);
    inf.guide = Some((origin, p, kind));
    inf
}

fn locked_plane(q: &Query, plane: Plane) -> Inference {
    if let Some(p) = point_snap(q) {
        let mut inf = base(plane.project(p.point), Kind::OnFace);
        inf.guide = Some((p.point, inf.point, p.kind));
        inf.plane = Some(plane);
        return inf;
    }
    let ray = q.ray();
    let t = plane.intersect_ray(&ray).or_else(|| plane.flipped().intersect_ray(&ray)).unwrap_or(0.0);
    let mut inf = base(ray.at(t), Kind::OnFace);
    inf.plane = Some(plane);
    inf
}

fn closest_on_line(origin: DVec3, dir: DVec3, p: DVec3) -> (DVec3, f64) {
    let s = (p - origin).dot(dir);
    (origin + dir * s, s)
}

/// The edge nearest the cursor on screen within the tolerance, ignoring hidden ones.
/// The Select and Eraser tools use it.
pub fn pick_edge(mesh: &Mesh, view: &View, cursor: DVec2, scale: f64) -> Option<EdgeId> {
    edge_snap(&Query::simple(mesh, view, cursor, scale)).and_then(|i| i.edge)
}

/// What a click selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// An edge, face, guide, or a group or component of the context.
    Element(Element),
    /// Geometry outside the context being edited (a click there closes the context).
    Outside,
    Nothing,
}

/// What a click at the cursor selects: the nearest edge or guide within the
/// tolerance, otherwise the face under the cursor; geometry of a group or component
/// selects the group or component.
pub fn pick(q: &Query) -> Pick {
    let classify = |inf: &Inference| {
        if let Some(i) = inf.instance {
            Some(Pick::Element(Element::Instance(i)))
        } else if let Some(e) = inf.edge {
            Some(Pick::Element(Element::Edge(e)))
        } else {
            inf.construction.map(|g| Pick::Element(Element::Guide(g)))
        }
    };
    if let Some(inf) = edge_snap(q) {
        return classify(&inf).unwrap_or(Pick::Outside);
    }
    // Guide points count as edges for picking.
    for (id, g) in q.guides {
        if let Some(p) = guide_endpoint(*g)
            && q.screen(p).is_some_and(|s| s.distance(q.cursor) <= POINT_PX * q.scale)
        {
            return Pick::Element(Element::Guide(*id));
        }
    }
    let ray = q.ray();
    let mut best: Option<(f64, Pick)> = None;
    for src in q.sources_on_ray(&ray, f64::INFINITY) {
        if let Some((f, t)) = src.pick_face(&ray)
            && t > 0.0
            && best.as_ref().is_none_or(|(bt, _)| t < *bt)
        {
            let p = match (src.active, src.instance) {
                (true, _) => Pick::Element(Element::Face(f)),
                (false, Some(i)) => Pick::Element(Element::Instance(i)),
                (false, None) => Pick::Outside,
            };
            best = Some((t, p));
        }
    }
    best.map(|(_, p)| p).unwrap_or(Pick::Nothing)
}

/// Re-export for tools that need the clamp helper with the same semantics.
pub fn closest_point_on_segment(a: DVec3, b: DVec3, p: DVec3) -> DVec3 {
    closest_on_segment(a, b, p).0
}

#[cfg(test)]
mod tests;
