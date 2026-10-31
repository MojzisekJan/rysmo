//! Modelling operations: erase, push/pull, move, copy, reverse and paint.

use std::collections::{BTreeMap, BTreeSet};

use rysmo_math::{DVec3, EPS, PolygonSide, Ray, point_in_polygon_2d};

use crate::ids::{EdgeId, Element, FaceId, MaterialId, VertexId};
use crate::mesh::{Face, Mesh, MeshError};
use crate::region::canonical;

/// What a push/pull made.
#[derive(Clone, Debug, PartialEq)]
pub struct PushPullResult {
    /// The pushed face, now at its new place, or `None` if it was pushed through.
    pub face: Option<FaceId>,
    /// The side faces that were created and survived merging.
    pub sides: Vec<FaceId>,
}

impl Mesh {
    /// Whether `p` lies inside a face's region (on its plane, inside the outline and
    /// outside its holes). Points on the boundary count as inside.
    pub fn face_contains(&self, f: FaceId, p: DVec3) -> bool {
        let Some(plane) = self.face_plane(f) else { return false };
        if !plane.contains(p) {
            return false;
        }
        let face = &self.faces[&f];
        let q = plane.to_2d(p);
        let poly = |l: &[VertexId]| l.iter().map(|v| plane.to_2d(self.pos(*v))).collect::<Vec<_>>();
        if point_in_polygon_2d(q, &poly(&face.outer), 1e-9) == PolygonSide::Outside {
            return false;
        }
        face.holes.iter().all(|h| point_in_polygon_2d(q, &poly(h), 1e-9) != PolygonSide::Inside)
    }

    /// The nearest face the ray hits, with the distance along the ray.
    pub fn pick_face(&self, ray: &Ray) -> Option<(FaceId, f64)> {
        self.pick_face_with_hidden(ray, false)
    }
    pub fn pick_face_with_hidden(&self, ray: &Ray, show_hidden: bool) -> Option<(FaceId, f64)> {
        self.pick_face_among(self.faces.keys().copied(), ray, show_hidden)
    }

    /// `pick_face_with_hidden` limited to some faces, given in id order.
    pub fn pick_face_among(
        &self,
        faces: impl IntoIterator<Item = FaceId>,
        ray: &Ray,
        show_hidden: bool,
    ) -> Option<(FaceId, f64)> {
        let mut best: Option<(FaceId, f64)> = None;
        for id in faces {
            if self.faces[&id].hidden && !show_hidden {
                continue;
            }
            let Some(plane) = self.face_plane(id) else { continue };
            let Some(t) = plane.intersect_ray(ray) else { continue };
            if best.is_some_and(|(_, bt)| t >= bt) {
                continue;
            }
            if self.face_contains(id, ray.at(t)) {
                best = Some((id, t));
            }
        }
        best
    }

    // ----- erase -----

    /// Erases elements as the Eraser does. Erasing an edge between two coplanar faces
    /// merges them; erasing any other edge also removes the faces it bounds.
    pub fn erase(&mut self, elements: &[Element]) {
        let affected: BTreeSet<_> = elements
            .iter()
            .filter_map(|e| match e {
                Element::Edge(id) => self.edges.get(id).and_then(|e| e.curve),
                _ => None,
            })
            .collect();
        let mut ends = BTreeSet::new();
        for e in elements {
            if let Element::Face(f) = e {
                self.faces.remove(f);
            }
        }
        for e in elements {
            if let Element::Edge(id) = e {
                let Some(edge) = self.edges.get(id) else { continue };
                ends.extend(edge.v);
                if self.merge_coplanar_at(*id) {
                    continue;
                }
                for f in self.faces_of_edge(*id) {
                    self.faces.remove(&f);
                }
                self.remove_edge_raw(*id);
            }
        }
        for v in ends {
            self.heal_vertex(v);
        }
        self.remove_orphan_vertices();
        for id in affected {
            if let Some(curve) = self.curves.get_mut(&id) {
                curve.kind = crate::CurveKind::Polyline;
            }
        }
        self.prune_curves();
    }

    /// If `e` sits between exactly two coplanar faces facing the same way, removes it so
    /// the faces become one. Returns whether it did.
    pub(crate) fn merge_coplanar_at(&mut self, e: EdgeId) -> bool {
        let faces = self.faces_of_edge(e);
        if faces.len() != 2 {
            return false;
        }
        let (Some(a), Some(b)) = (self.face_plane(faces[0]), self.face_plane(faces[1])) else { return false };
        if !a.same_surface(&b) || a.normal.dot(b.normal) <= 0.0 {
            return false;
        }
        let [v0, v1] = self.edges[&e].v;
        self.remove_edge_raw(e);
        self.rebuild_plane(&a, &BTreeSet::new(), None);
        self.heal_vertex(v0);
        self.heal_vertex(v1);
        true
    }

    /// Removes a vertex that only joins two collinear edges, making them one edge.
    pub(crate) fn heal_vertex(&mut self, v: VertexId) {
        if !self.vertices.contains_key(&v) {
            return;
        }
        let edges = self.edges_of_vertex(v);
        if edges.len() != 2 {
            return;
        }
        let other = |m: &Mesh, e: EdgeId| {
            let [a, b] = m.edges[&e].v;
            if a == v { b } else { a }
        };
        if self.edges[&edges[0]].material != self.edges[&edges[1]].material {
            return;
        }
        let a = other(self, edges[0]);
        let b = other(self, edges[1]);
        let p = self.pos(v);
        let da = (self.pos(a) - p).normalize();
        let db = (self.pos(b) - p).normalize();
        if da.dot(db) > -1.0 + 1e-12 || !rysmo_math::parallel(da, db) || self.edge_between(a, b).is_some() {
            return;
        }
        let flags = self.edges[&edges[0]].flags();
        let curve = self.edges[&edges[0]].curve.filter(|id| self.edges[&edges[1]].curve == Some(*id));
        self.remove_edge_raw(edges[0]);
        self.remove_edge_raw(edges[1]);
        let joined = self.raw_edge(a, b);
        let edge = self.edges.get_mut(&joined).expect("just made");
        edge.set_flags(flags);
        edge.curve = curve;
        for id in self.faces_of_vertex(v) {
            let f = self.faces.get_mut(&id).expect("just listed");
            for l in std::iter::once(&mut f.outer).chain(f.holes.iter_mut()) {
                l.retain(|x| *x != v);
            }
        }
        self.vertices.remove(&v);
    }

    // ----- push/pull -----

    /// Pushes or pulls a face along its normal by `distance` (negative pushes in).
    ///
    /// Side faces join the old and new outline; a face standing alone
    /// (no other face along its edges) leaves a copy behind so the result is closed, as
    /// does `keep_base` (Ctrl in the tool); side faces coplanar with a neighbour merge
    /// into it; and a face pushed onto an opposite face it fits inside cuts a hole
    /// through.
    pub fn push_pull(&mut self, f: FaceId, distance: f64, keep_base: bool) -> Result<PushPullResult, MeshError> {
        if distance.abs() < EPS {
            return Err(MeshError::ZeroDistance);
        }
        let plane = self.face_plane(f).ok_or(MeshError::Missing(f.0))?;
        let offset = plane.normal * distance;
        if !keep_base && let Some(result) = self.collapse_extrusion(f, offset) {
            return Ok(result);
        }
        let face = self.faces[&f].clone();
        let boundary = self.face_edges(f);
        let polygon_vertices: BTreeSet<_> = boundary
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
        let smooth_vertices: BTreeSet<_> = face
            .loops()
            .flatten()
            .copied()
            .filter(|v| {
                let mut ids = BTreeMap::<crate::CurveId, usize>::new();
                for edge in &boundary {
                    let e = &self.edges[edge];
                    if e.v.contains(v)
                        && let Some(id) = e.curve
                        && self.curves.get(&id).is_some_and(|c| !c.faceted && c.kind != crate::CurveKind::Polygon)
                    {
                        *ids.entry(id).or_default() += 1;
                    }
                }
                ids.values().any(|count| *count >= 2)
            })
            .collect();
        let curve_ids: BTreeSet<_> = boundary.iter().filter_map(|e| self.edges[e].curve).collect();
        let boundary_curves: Vec<_> =
            curve_ids.iter().map(|id| (*id, self.curves[id], self.curve_edges(*id))).collect();
        let free = boundary.iter().all(|e| self.faces_of_edge(*e) == [f]);
        let keep_base = keep_base || free;

        let mut map: BTreeMap<VertexId, VertexId> = BTreeMap::new();
        for l in face.loops() {
            for v in l {
                let p = self.pos(*v) + offset;
                let n = self.add_vertex(p);
                map.insert(*v, n);
            }
        }
        for (a, b) in face.loop_edges() {
            let flags = self.edges[&self.edge_between(a, b).expect("face edge")].flags();
            let edge = self.raw_edge(map[&a], map[&b]);
            self.edges.get_mut(&edge).unwrap().set_flags(flags);
        }
        for (_, curve, members) in &boundary_curves {
            let mapped: Vec<_> = members
                .iter()
                .filter_map(|id| {
                    let [a, b] = self.edges.get(id)?.v;
                    self.edge_between(*map.get(&a)?, *map.get(&b)?)
                })
                .collect();
            if !mapped.is_empty() {
                let mut descriptor = curve.mapped(&|p| p + offset);
                if mapped.len() != members.len() {
                    descriptor.kind = crate::CurveKind::Polyline;
                }
                self.register_curve(&mapped, descriptor)?;
            }
        }
        for (v, n) in &map {
            self.raw_edge(*v, *n);
        }
        // The face moves; its id and material stay.
        let moved = Face {
            hidden: face.hidden,
            outer: face.outer.iter().map(|v| map[v]).collect(),
            holes: face.holes.iter().map(|h| h.iter().map(|v| map[v]).collect()).collect(),
            material: face.material,
            back_material: face.back_material,
            uv: face.uv,
        };
        self.faces.insert(f, moved);
        // Pulling a free face backwards (a face on the ground faces down, and is pulled
        // up) builds a solid behind it: the moved face and the sides turn round, and the
        // face left behind keeps its facing.
        let flip = distance < 0.0 && free;
        if flip {
            self.faces.get_mut(&f).expect("just inserted").reverse();
        }
        let mut sides = Vec::new();
        for (a, b) in face.loop_edges() {
            let mut quad = vec![a, b, map[&b], map[&a]];
            if flip {
                quad.reverse();
            }
            sides.push(self.raw_face(Face {
                hidden: false,
                outer: quad,
                holes: Vec::new(),
                material: None,
                back_material: None,
                uv: None,
            }));
        }
        if keep_base {
            let mut base = Face { material: None, back_material: None, uv: None, ..face.clone() };
            if distance > 0.0 {
                base.reverse();
            }
            self.raw_face(base);
        }
        for e in boundary {
            if self.edges.contains_key(&e) {
                self.merge_coplanar_at(e);
            }
        }
        // Edges between the new sides that now continue a merged face may be redundant.
        let verticals: Vec<VertexId> = map.keys().copied().collect();
        for v in verticals {
            self.heal_vertex(v);
        }

        // Side edges between nearly flush sides (a pulled circle) turn soft and smooth,
        // so a cylinder looks round.
        let verticals: Vec<EdgeId> = map
            .iter()
            .filter(|(v, _)| !polygon_vertices.contains(v))
            .filter_map(|(v, n)| self.edge_between(*v, *n))
            .collect();
        self.soften_by_angle(&verticals, crate::SOFTEN_ANGLE_DEG);
        let curved: Vec<_> = map
            .iter()
            .filter(|(v, _)| smooth_vertices.contains(v))
            .filter_map(|(v, n)| self.edge_between(*v, *n))
            .collect();
        self.set_edge_flags(&curved, Some(true), Some(true), None);

        let mut result_face = Some(f);
        if let Some(cut) = self.opposite_host(f) {
            result_face = None;
            let cap_plane = self.face_plane(f).expect("cap is planar");
            let cap_outer = canonical(&self.faces[&f].outer);
            let seeds: BTreeSet<EdgeId> = self.face_edges(f).into_iter().collect();
            self.faces.remove(&f);
            let _ = cut;
            self.rebuild_plane(&cap_plane, &seeds, None);
            let inner: Vec<FaceId> =
                self.faces.iter().filter(|(_, g)| canonical(&g.outer) == cap_outer).map(|(id, _)| *id).collect();
            for g in inner {
                self.faces.remove(&g);
            }
        }
        let sides = sides.into_iter().filter(|s| self.faces.contains_key(s)).collect();
        Ok(PushPullResult { face: result_face, sides })
    }

    /// Returning an extrusion to its base removes the side shell rather than
    /// constructing overlapping reverse walls or punching away the base face.
    fn collapse_extrusion(&mut self, face_id: FaceId, offset: DVec3) -> Option<PushPullResult> {
        let face = self.faces.get(&face_id)?.clone();
        let vertices: BTreeSet<_> = face.loops().flatten().copied().collect();
        let mapped: Option<BTreeMap<_, _>> = vertices
            .iter()
            .map(|v| {
                let target = self.find_vertex(self.pos(*v) + offset)?;
                (!vertices.contains(&target)).then_some((*v, target))
            })
            .collect();
        let mapped = mapped?;
        let targets: BTreeSet<_> = mapped.values().copied().collect();
        let allowed: BTreeSet<_> = vertices.union(&targets).copied().collect();
        let boundary = self.face_edges(face_id);
        let mut sides = BTreeSet::new();
        for edge in boundary {
            let adjacent: Vec<_> = self.faces_of_edge(edge).into_iter().filter(|id| *id != face_id).collect();
            if adjacent.len() != 1 {
                return None;
            }
            let side = &self.faces[&adjacent[0]];
            if !side.loops().flatten().all(|v| allowed.contains(v))
                || !side.loops().flatten().any(|v| targets.contains(v))
            {
                return None;
            }
            sides.insert(adjacent[0]);
        }
        if vertices.iter().flat_map(|v| self.faces_of_vertex(*v)).any(|id| id != face_id && !sides.contains(&id)) {
            return None;
        }
        let shell_edges: BTreeSet<_> =
            std::iter::once(face_id).chain(sides.iter().copied()).flat_map(|face| self.face_edges(face)).collect();
        if vertices.iter().flat_map(|v| self.edges_of_vertex(*v)).any(|edge| !shell_edges.contains(&edge)) {
            return None;
        }
        let mut flat = face;
        flat.outer.iter_mut().for_each(|v| *v = mapped[v]);
        for hole in &mut flat.holes {
            hole.iter_mut().for_each(|v| *v = mapped[v]);
        }
        let flat_plane = rysmo_math::Plane::from_polygon(&self.loop_points(&flat.outer))?;
        let target_points: Vec<_> = flat.outer.iter().map(|v| self.pos(*v)).collect();
        let existing = self
            .faces
            .iter()
            .find(|(id, other)| {
                **id != face_id
                    && !sides.contains(id)
                    && canonical(&other.outer) == canonical(&flat.outer)
                    && other.holes.len() == flat.holes.len()
                    && other.holes.iter().all(|hole| flat.holes.iter().any(|h| canonical(h) == canonical(hole)))
            })
            .map(|(id, _)| *id);
        self.faces.remove(&face_id);
        for side in sides {
            self.faces.remove(&side);
        }
        let discarded: Vec<_> = self
            .edges
            .iter()
            .filter(|(_, edge)| edge.v.iter().any(|v| vertices.contains(v)))
            .map(|(id, _)| *id)
            .collect();
        for edge in discarded {
            self.remove_edge_raw(edge);
        }
        let result = if let Some(existing) = existing {
            existing
        } else {
            self.faces.insert(face_id, flat);
            let boundaries = self.face_edges(face_id);
            for edge in boundaries {
                if self.edges.contains_key(&edge) {
                    self.merge_coplanar_at(edge);
                }
            }
            face_id
        };
        self.remove_orphan_vertices();
        self.prune_curves();
        let face = if self.faces.contains_key(&result) {
            Some(result)
        } else {
            self.faces.keys().copied().find(|id| {
                self.face_plane(*id).is_some_and(|p| p.same_surface(&flat_plane))
                    && target_points.iter().all(|p| self.face_contains(*id, *p))
            })
        };
        Some(PushPullResult { face, sides: Vec::new() })
    }

    /// A face coplanar with `f`, facing the other way, whose region holds all of `f`.
    fn opposite_host(&self, f: FaceId) -> Option<FaceId> {
        let plane = self.face_plane(f)?;
        let verts: Vec<DVec3> = self.faces[&f].outer.iter().map(|v| self.pos(*v)).collect();
        self.faces.keys().copied().find(|g| {
            *g != f
                && self.face_plane(*g).is_some_and(|gp| gp.same_surface(&plane) && gp.normal.dot(plane.normal) < 0.0)
                && verts.iter().all(|p| self.face_contains(*g, *p))
        })
    }

    // ----- faces -----

    /// Turns faces round, swapping their front and back materials.
    pub fn reverse_faces(&mut self, faces: &[FaceId]) {
        for f in faces {
            if let Some(face) = self.faces.get_mut(f) {
                face.reverse();
                std::mem::swap(&mut face.material, &mut face.back_material);
            }
        }
    }

    /// Paints the front or back of a face.
    pub fn paint(&mut self, f: FaceId, material: Option<MaterialId>, back: bool) -> Result<(), MeshError> {
        let face = self.faces.get_mut(&f).ok_or(MeshError::Missing(f.0))?;
        if back {
            face.back_material = material;
        } else {
            face.material = material;
        }
        Ok(())
    }
}

impl Mesh {
    /// Adds a strictly interior opening to a surface copy. Boundary IDs are new and
    /// hidden: a glued component supplies the visible outline.
    pub fn with_surface_opening(&self, face: FaceId, points: &[DVec3]) -> Result<Mesh, MeshError> {
        use rysmo_math::{Plane, PolygonSide, point_in_polygon_2d, segment_intersection};
        let invalid = || {
            MeshError::Invalid(
                "the opening must be a simple loop wholly inside one face, without overlapping another opening".into(),
            )
        };
        let host = self.face(face).ok_or_else(invalid)?;
        let plane = self.face_plane(face).ok_or_else(invalid)?;
        if points.len() < 3 || points.iter().any(|p| !p.is_finite() || plane.distance(*p).abs() > 1e-7) {
            return Err(invalid());
        }
        let polygon: Vec<_> = points.iter().map(|p| plane.to_2d(*p)).collect();
        let outer: Vec<_> = host.outer.iter().map(|v| plane.to_2d(self.pos(*v))).collect();
        if polygon.iter().any(|p| point_in_polygon_2d(*p, &outer, 1e-8) != PolygonSide::Inside) {
            return Err(invalid());
        }
        for hole in &host.holes {
            let old: Vec<_> = hole.iter().map(|v| plane.to_2d(self.pos(*v))).collect();
            if polygon.iter().any(|p| point_in_polygon_2d(*p, &old, 1e-8) != PolygonSide::Outside)
                || old.iter().any(|p| point_in_polygon_2d(*p, &polygon, 1e-8) != PolygonSide::Outside)
            {
                return Err(invalid());
            }
        }
        for i in 0..points.len() {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            if a.distance(b) < 1e-8 {
                return Err(invalid());
            }
            for (c, d) in host.loop_edges().into_iter().map(|(c, d)| (self.pos(c), self.pos(d))) {
                if segment_intersection(a, b, c, d).is_some() {
                    return Err(invalid());
                }
            }
            for j in (i + 1)..points.len() {
                if j == i + 1 || (i == 0 && j == points.len() - 1) {
                    continue;
                }
                if segment_intersection(a, b, points[j], points[(j + 1) % points.len()]).is_some() {
                    return Err(invalid());
                }
            }
        }
        let outline_plane = Plane::from_polygon(points).ok_or_else(invalid)?;
        let mut mesh = self.clone();
        let mut vertices: Vec<_> = points.iter().map(|p| mesh.add_vertex(*p)).collect();
        if outline_plane.normal.dot(plane.normal) > 0. {
            vertices.reverse();
        }
        for i in 0..vertices.len() {
            let edge = mesh.raw_edge(vertices[i], vertices[(i + 1) % vertices.len()]);
            mesh.edges.get_mut(&edge).unwrap().hidden = true;
            mesh.edges.get_mut(&edge).unwrap().procedural = true;
        }
        mesh.faces.get_mut(&face).unwrap().holes.push(vertices);
        mesh.validate()?;
        Ok(mesh)
    }
}
