//! Moving, rotating, scaling and copying geometry, with auto-fold, and
//! moving geometry between meshes (making groups and exploding them).

use std::collections::{BTreeMap, BTreeSet};

use rysmo_math::{DVec3, EPS, Plane};

use crate::ids::{EdgeId, Element, FaceId, MaterialId, VertexId};
use crate::mesh::{Face, Mesh, MeshError};
use crate::region::canonical;

/// Display attributes preserved when an edge is copied into another mesh.
#[derive(Clone, Debug)]
struct EdgeAppearance {
    soft: bool,
    smooth: bool,
    hidden: bool,
    material: Option<MaterialId>,
}

#[derive(Clone, Debug)]
struct TemplateLine {
    start: DVec3,
    end: DVec3,
    appearance: EdgeAppearance,
}

#[derive(Clone, Debug)]
struct TemplateCurve {
    curve: crate::Curve,
    segments: Vec<(DVec3, DVec3)>,
}

#[derive(Clone, Debug)]
struct TemplateFace {
    outer: Vec<DVec3>,
    holes: Vec<Vec<DVec3>>,
    normal: DVec3,
    material: Option<MaterialId>,
    back_material: Option<MaterialId>,
    uv: Option<crate::TexturePlacement>,
    hidden: bool,
}

/// Geometry lifted out of a mesh so it can be stamped somewhere else, transformed.
#[derive(Clone, Debug, Default)]
pub struct Template {
    lines: Vec<TemplateLine>,
    curves: Vec<TemplateCurve>,
    faces: Vec<TemplateFace>,
}

impl Template {
    /// The edges and faces of `elements` (faces bring their edges).
    pub fn of(mesh: &Mesh, elements: &[Element]) -> Template {
        let mut edges: BTreeSet<EdgeId> = BTreeSet::new();
        let mut t = Template::default();
        for e in elements {
            match e {
                Element::Edge(id) if mesh.edges.contains_key(id) => {
                    edges.insert(*id);
                }
                Element::Face(id) if mesh.faces.contains_key(id) => {
                    edges.extend(mesh.face_edges(*id));
                    let f = &mesh.faces[id];
                    let n = mesh.face_normal(*id).unwrap_or(DVec3::Z);
                    t.faces.push(TemplateFace {
                        outer: mesh.loop_points(&f.outer),
                        holes: f.holes.iter().map(|h| mesh.loop_points(h)).collect(),
                        normal: n,
                        material: f.material,
                        back_material: f.back_material,
                        uv: f.uv,
                        hidden: f.hidden,
                    });
                }
                _ => {}
            }
        }
        let curve_ids: BTreeSet<_> = edges.iter().filter_map(|id| mesh.edges[id].curve).collect();
        for id in curve_ids {
            let all = mesh.curve_edges(id);
            let members: Vec<_> = all.iter().copied().filter(|id| edges.contains(id)).collect();
            let mut curve = mesh.curves[&id];
            if members.len() != all.len() {
                curve.kind = crate::CurveKind::Polyline;
            }
            t.curves.push(TemplateCurve { curve, segments: members.iter().map(|e| mesh.edge_points(*e)).collect() });
        }
        for e in edges {
            let (a, b) = mesh.edge_points(e);
            let edge = &mesh.edges[&e];
            t.lines.push(TemplateLine {
                start: a,
                end: b,
                appearance: EdgeAppearance {
                    soft: edge.soft,
                    smooth: edge.smooth,
                    hidden: edge.hidden,
                    material: edge.material,
                },
            });
        }
        t
    }

    /// Everything in a mesh.
    pub fn whole(mesh: &Mesh) -> Template {
        let all: Vec<Element> =
            mesh.edges.keys().map(|e| Element::Edge(*e)).chain(mesh.faces.keys().map(|f| Element::Face(*f))).collect();
        Template::of(mesh, &all)
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

impl Mesh {
    /// Applies `f` to vertex positions, stretching what is attached. A face that would
    /// stop being flat is automatically folded along new edges: it is
    /// split into triangles (worked out before the move, so they are valid) and the
    /// triangles that stay coplanar are merged back. Fails without changing anything if
    /// a face would collapse. Vertices that land on others weld.
    pub fn transform_vertices(
        &mut self,
        vertices: &BTreeSet<VertexId>,
        f: impl Fn(DVec3) -> DVec3,
    ) -> Result<(), MeshError> {
        let mut m = self.clone();
        m.map_touched_curves(vertices, &f);
        for v in vertices {
            if let Some(vx) = m.vertices.get_mut(v) {
                vx.pos = f(vx.pos);
            }
        }
        let touched: BTreeSet<FaceId> = vertices.iter().flat_map(|v| self.faces_of_vertex(*v)).collect();
        let mut diagonals: Vec<EdgeId> = Vec::new();
        for face in &touched {
            if m.face_is_flat(*face) {
                continue;
            }
            let tris = crate::triangulate_face(self, *face);
            let old = m.faces.remove(face).expect("touched face exists");
            for t in tris {
                for k in 0..3 {
                    let (a, b) = (t[k], t[(k + 1) % 3]);
                    if m.edge_between(a, b).is_none() {
                        diagonals.push(m.raw_edge(a, b));
                    }
                }
                m.raw_face(Face {
                    hidden: old.hidden,
                    outer: t.to_vec(),
                    holes: Vec::new(),
                    material: old.material,
                    back_material: old.back_material,
                    uv: old.uv,
                });
            }
        }
        for e in diagonals {
            if m.edges.contains_key(&e) {
                m.merge_coplanar_at(e);
            }
        }
        let now_touched: BTreeSet<FaceId> = vertices.iter().flat_map(|v| m.faces_of_vertex(*v)).collect();
        for face in now_touched {
            if !m.face_is_flat(face) {
                return Err(MeshError::WouldBend(face));
            }
        }
        let moved_lines: Vec<_> = m
            .edges
            .values()
            .filter(|edge| edge.v.iter().any(|v| vertices.contains(v)))
            .map(|edge| (m.pos(edge.v[0]), m.pos(edge.v[1])))
            .collect();
        for v in vertices {
            if !m.vertices.contains_key(v) {
                continue;
            }
            let p = m.pos(*v);
            let other = m
                .vertices
                .iter()
                .find(|(id, x)| *id != v && !vertices.contains(id) && x.pos.distance(p) < EPS)
                .map(|(id, _)| *id);
            if let Some(o) = other {
                m.weld(*v, o);
            }
        }
        // Welding can leave two faces on the same loop (both halves of a fold); keep one.
        let mut seen = BTreeSet::new();
        let dupes: Vec<FaceId> =
            m.faces.iter().filter(|(_, f)| !seen.insert(canonical(&f.outer))).map(|(id, _)| *id).collect();
        for f in dupes {
            m.faces.remove(&f);
        }
        m.split_moved_crossings(&moved_lines);
        m.prune_curves();
        *self = m;
        Ok(())
    }

    /// Sticky geometry also intersects edges which meet between their endpoints.
    /// Split in the candidate mesh so a failed transform never leaks partial edits.
    fn split_moved_crossings(&mut self, lines: &[(DVec3, DVec3)]) {
        use rysmo_math::{closest_on_segment, segment_intersection};
        let affected: BTreeSet<_> = lines.iter().flat_map(|(a, b)| self.edges_along(*a, *b)).collect();
        let mut points = Vec::new();
        for id in &affected {
            let (a, b) = self.edge_points(*id);
            for (other, edge) in &self.edges {
                if other == id || (affected.contains(other) && other < id) {
                    continue;
                }
                let c = self.pos(edge.v[0]);
                let d = self.pos(edge.v[1]);
                if (a.max(b) + DVec3::splat(EPS)).cmplt(c.min(d)).any()
                    || (c.max(d) + DVec3::splat(EPS)).cmplt(a.min(b)).any()
                {
                    continue;
                }
                if let Some((t, _)) = segment_intersection(a, b, c, d) {
                    let point = a + (b - a) * t;
                    let interior = (point.distance(a) > EPS && point.distance(b) > EPS)
                        || (point.distance(c) > EPS && point.distance(d) > EPS);
                    if interior && !points.iter().any(|p: &DVec3| p.distance(point) < EPS) {
                        points.push(point);
                    }
                }
            }
        }
        let mut seeds = BTreeSet::new();
        for point in points {
            let crossing: Vec<_> = self
                .edges
                .iter()
                .filter_map(|(id, edge)| {
                    let a = self.pos(edge.v[0]);
                    let b = self.pos(edge.v[1]);
                    (point.distance(a) > EPS
                        && point.distance(b) > EPS
                        && closest_on_segment(a, b, point).0.distance(point) < EPS)
                        .then_some(*id)
                })
                .collect();
            if crossing.is_empty() {
                continue;
            }
            let vertex = self.add_vertex(point);
            for edge in crossing {
                let (a, b) = self.split_edge(edge, vertex);
                seeds.extend([a, b]);
            }
        }
        if !seeds.is_empty() {
            seeds.extend(affected.into_iter().filter(|id| self.edges.contains_key(id)));
            self.rebuild_around(&seeds.into_iter().collect::<Vec<_>>(), None);
        }
    }

    /// Whether a face is flat and not degenerate.
    pub(crate) fn face_is_flat(&self, f: FaceId) -> bool {
        let face = &self.faces[&f];
        let Some(plane) = Plane::from_polygon(&self.loop_points(&face.outer)) else { return false };
        face.loops().flatten().all(|v| plane.distance(self.pos(*v)).abs() <= EPS * 10.0)
    }

    /// Deforms existing geometry without welding, retriangulating or changing IDs.
    /// Rejects collapsed/non-planar faces atomically; intended for triangulated terrain.
    pub fn deform_vertices(
        &mut self,
        vertices: &BTreeSet<VertexId>,
        f: impl Fn(DVec3) -> DVec3,
    ) -> Result<(), MeshError> {
        if vertices.iter().any(|id| !self.vertices.contains_key(id)) {
            return Err(MeshError::Invalid("deformation vertex is missing".into()));
        }
        let mut candidate = self.clone();
        candidate.map_touched_curves(vertices, &f);
        for id in vertices {
            let vertex = candidate.vertices.get_mut(id).unwrap();
            vertex.pos = f(vertex.pos);
            if !vertex.pos.is_finite() {
                return Err(MeshError::Invalid("deformation produced a non-finite point".into()));
            }
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub fn move_vertices(&mut self, vertices: &BTreeSet<VertexId>, offset: DVec3) -> Result<(), MeshError> {
        self.transform_vertices(vertices, |p| p + offset)
    }

    /// Moves elements: the vertices they touch.
    pub fn move_elements(&mut self, elements: &[Element], offset: DVec3) -> Result<(), MeshError> {
        let vs = self.vertices_of(elements);
        self.move_vertices(&vs, offset)
    }

    /// Applies `f` to the vertices of elements (Rotate, Scale).
    pub fn transform_elements(&mut self, elements: &[Element], f: impl Fn(DVec3) -> DVec3) -> Result<(), MeshError> {
        let vs = self.vertices_of(elements);
        self.transform_vertices(&vs, f)
    }

    /// Copies elements `copies` times, each `offset` further on, as Move with Ctrl does
    /// (and "5x" in the VCB). The copies stick to whatever they touch.
    pub fn copy_elements(
        &mut self,
        elements: &[Element],
        offset: DVec3,
        copies: usize,
    ) -> Result<Vec<Element>, MeshError> {
        if offset.length() < EPS {
            return Err(MeshError::ZeroLength);
        }
        self.copy_elements_with(elements, copies, |p, k| p + offset * k as f64)
    }

    /// Copies elements `copies` times, the k-th copy mapped by `f(p, k)`.
    pub fn copy_elements_with(
        &mut self,
        elements: &[Element],
        copies: usize,
        f: impl Fn(DVec3, usize) -> DVec3,
    ) -> Result<Vec<Element>, MeshError> {
        let t = Template::of(self, elements);
        let mut out = Vec::new();
        for k in 1..=copies.max(1) {
            out.extend(self.stamp(&t, |p| f(p, k))?);
        }
        Ok(out)
    }

    /// Draws a template's lines mapped by `f`, then gives the faces that formed the
    /// template's facing, materials and edge flags. Returns the new elements.
    pub fn stamp(&mut self, t: &Template, f: impl Fn(DVec3) -> DVec3) -> Result<Vec<Element>, MeshError> {
        let mut out = Vec::new();
        let lines: Vec<(DVec3, DVec3)> =
            t.lines.iter().map(|line| (f(line.start), f(line.end))).filter(|(a, b)| a.distance(*b) >= EPS).collect();
        if lines.is_empty() {
            return Ok(out);
        }
        let has_holes =
            t.faces.iter().any(|face| !face.holes.is_empty()) || self.faces.values().any(|face| !face.holes.is_empty());
        let existing = has_holes.then(|| self.clone());
        let before: BTreeSet<_> = self.faces.keys().copied().collect();
        let edges = self.add_lines(&lines, None)?;
        out.extend(edges.into_iter().map(Element::Edge));
        for line in &t.lines {
            let appearance = &line.appearance;
            if !appearance.soft && !appearance.smooth && !appearance.hidden && appearance.material.is_none() {
                continue;
            }
            if let (Some(x), Some(y)) = (self.find_vertex(f(line.start)), self.find_vertex(f(line.end)))
                && let Some(e) = self.edge_between(x, y)
            {
                let edge = self.edges.get_mut(&e).ok_or(MeshError::Missing(e.0))?;
                edge.soft = appearance.soft;
                edge.smooth = appearance.smooth;
                edge.hidden = appearance.hidden;
                edge.material = appearance.material;
            }
        }
        for template in &t.curves {
            let lines: Vec<_> = template.segments.iter().map(|(a, b)| (f(*a), f(*b))).collect();
            let members = self.edges_on_segments(&lines);
            if !members.is_empty() {
                self.register_curve(&members, template.curve.mapped(&f))?;
            }
        }
        self.prune_curves();
        if has_holes {
            let mut regions = t
                .faces
                .iter()
                .filter_map(|face| {
                    let mapped = face.outer.iter().map(|p| f(*p)).collect::<Vec<_>>();
                    let plane = Plane::from_polygon(&mapped)?;
                    let (u, v) = plane.basis();
                    let project = |p: DVec3| rysmo_math::DVec2::new(p.dot(u), p.dot(v));
                    Some((
                        plane,
                        mapped.into_iter().map(project).collect::<Vec<_>>(),
                        face.holes
                            .iter()
                            .map(|h| h.iter().map(|p| project(f(*p))).collect::<Vec<_>>())
                            .collect::<Vec<_>>(),
                    ))
                })
                .collect::<Vec<_>>();
            if let Some(existing) = &existing {
                for (id, face) in existing.faces().filter(|(_, face)| !face.holes.is_empty()) {
                    let Some(plane) = existing.face_plane(id) else { continue };
                    let (u, v) = plane.basis();
                    let project = |p: DVec3| rysmo_math::DVec2::new(p.dot(u), p.dot(v));
                    regions.push((
                        plane,
                        face.outer.iter().map(|v| project(existing.pos(*v))).collect(),
                        face.holes
                            .iter()
                            .map(|hole| hole.iter().map(|v| project(existing.pos(*v))).collect())
                            .collect(),
                    ));
                }
            }
            let remove = self
                .faces
                .iter()
                .filter(|(id, _)| !before.contains(id))
                .filter_map(|(id, face)| {
                    let plane = self.face_plane(*id)?;
                    let centre = self.loop_points(&face.outer).iter().copied().sum::<DVec3>() / face.outer.len() as f64;
                    let point = if self.face_contains(*id, centre) {
                        centre
                    } else {
                        face.outer.iter().zip(face.outer.iter().cycle().skip(1)).find_map(|(a, b)| {
                            let a = self.pos(*a);
                            let b = self.pos(*b);
                            let p = (a + b) * 0.5 + plane.normal.cross(b - a).normalize() * EPS * 10.;
                            self.face_contains(*id, p).then_some(p)
                        })?
                    };
                    let mut in_hole = false;
                    let mut covered =
                        existing.as_ref().is_some_and(|mesh| mesh.faces().any(|(id, _)| mesh.face_contains(id, point)));
                    for (plane, outer, holes) in &regions {
                        if plane.distance(point).abs() > EPS {
                            continue;
                        }
                        let (u, v) = plane.basis();
                        let p = rysmo_math::DVec2::new(point.dot(u), point.dot(v));
                        let hole = holes
                            .iter()
                            .any(|h| rysmo_math::point_in_polygon_2d(p, h, EPS) == rysmo_math::PolygonSide::Inside);
                        in_hole |= hole;
                        covered |=
                            rysmo_math::point_in_polygon_2d(p, outer, EPS) == rysmo_math::PolygonSide::Inside && !hole;
                    }
                    (in_hole && !covered).then_some(*id)
                })
                .collect::<Vec<_>>();
            for face in remove {
                self.faces.remove(&face);
            }
        }
        for face in &t.faces {
            let mapped: Vec<DVec3> = face.outer.iter().map(|p| f(*p)).collect();
            let ids: Option<Vec<VertexId>> = mapped.iter().map(|p| self.find_vertex(*p)).collect();
            let Some(ids) = ids else { continue };
            let want = canonical(&ids);
            let found = self.faces.iter().find(|(_, g)| {
                let c = canonical(&g.outer);
                c == want || (c.len() >= want.len() && want.iter().all(|v| g.outer.contains(v)))
            });
            let Some((id, _)) = found else { continue };
            let id = *id;
            // Map the normal as a direction from the outline's centre: that keeps the
            // copy facing the right side, mirror images included.
            let c = face.outer.iter().copied().sum::<DVec3>() / face.outer.len() as f64;
            let want_normal = Some(f(c + face.normal) - f(c));
            if let (Some(n), Some(w)) = (self.face_normal(id), want_normal)
                && n.dot(w) < 0.0
            {
                self.faces.get_mut(&id).ok_or(MeshError::Missing(id.0))?.reverse();
            }
            let g = self.faces.get_mut(&id).ok_or(MeshError::Missing(id.0))?;
            g.material = face.material;
            g.back_material = face.back_material;
            g.hidden = face.hidden;
            g.uv = face.uv;
            out.push(Element::Face(id));
        }
        Ok(out)
    }

    /// Takes elements out of the mesh into a new mesh, with fresh ids from this mesh's
    /// counter (the caller syncs the document counter). Edges still used by faces that
    /// stay are copied rather than moved. Faces bring their edges.
    pub fn extract(&mut self, elements: &[Element]) -> Mesh {
        let mut faces: BTreeSet<FaceId> = BTreeSet::new();
        let mut edges: BTreeSet<EdgeId> = BTreeSet::new();
        for e in elements {
            match e {
                Element::Face(f) if self.faces.contains_key(f) => {
                    faces.insert(*f);
                    edges.extend(self.face_edges(*f));
                }
                Element::Edge(id) if self.edges.contains_key(id) => {
                    edges.insert(*id);
                }
                _ => {}
            }
        }
        let mut out = Mesh::new();
        out.next_id = self.next_id;
        let mut map: BTreeMap<VertexId, VertexId> = BTreeMap::new();
        let mut vmap =
            |out: &mut Mesh, m: &Mesh, v: VertexId| *map.entry(v).or_insert_with(|| out.add_vertex(m.pos(v)));
        let mut curve_map = BTreeMap::new();
        for e in &edges {
            let [a, b] = self.edges[e].v;
            let (x, y) = (vmap(&mut out, self, a), vmap(&mut out, self, b));
            let n = out.raw_edge(x, y);
            out.edges.get_mut(&n).expect("just made").set_flags(self.edges[e].flags());
            if let Some(curve) = self.edges[e].curve {
                let mapped = *curve_map.entry(curve).or_insert_with(|| {
                    let id = crate::CurveId(out.reserve_id());
                    let mut descriptor = self.curves[&curve];
                    if self.curve_edges(curve).iter().any(|id| !edges.contains(id)) {
                        descriptor.kind = crate::CurveKind::Polyline;
                    }
                    out.curves.insert(id, descriptor);
                    id
                });
                out.edges.get_mut(&n).unwrap().curve = Some(mapped);
            }
        }
        for f in &faces {
            let face = &self.faces[f];
            let mapped = Face {
                hidden: face.hidden,
                outer: face.outer.iter().map(|v| vmap(&mut out, self, *v)).collect(),
                holes: face.holes.iter().map(|h| h.iter().map(|v| vmap(&mut out, self, *v)).collect()).collect(),
                material: face.material,
                back_material: face.back_material,
                uv: face.uv,
            };
            out.raw_face(mapped);
        }
        for f in &faces {
            self.faces.remove(f);
        }
        for e in &edges {
            if self.faces_of_edge(*e).is_empty() {
                self.remove_edge_raw(*e);
            }
        }
        self.remove_orphan_vertices();
        self.prune_curves();
        self.next_id = out.next_id;
        out
    }

    /// Applies `f` to every vertex (moving a whole definition's geometry, for example
    /// to put its origin somewhere else). No folding: `f` should be affine.
    pub fn map_all(&mut self, f: impl Fn(DVec3) -> DVec3) {
        for curve in self.curves.values_mut() {
            *curve = curve.mapped(&f);
        }
        for v in self.vertices.values_mut() {
            v.pos = f(v.pos);
        }
    }
}

#[cfg(test)]
mod template_tests {
    use super::*;
    use crate::{Curve, CurveKind, TexturePlacement};

    #[test]
    fn mirrored_template_preserves_holes_materials_uv_curves_and_visibility() {
        let mut source = Mesh::new();
        for ring in [
            [DVec3::ZERO, DVec3::X * 4., DVec3::new(4., 4., 0.), DVec3::Y * 4.],
            [DVec3::new(1., 1., 0.), DVec3::new(3., 1., 0.), DVec3::new(3., 3., 0.), DVec3::new(1., 3., 0.)],
        ] {
            let lines: Vec<_> = ring.iter().zip(ring.iter().cycle().skip(1)).take(4).map(|(a, b)| (*a, *b)).collect();
            source.add_lines(&lines, Some(DVec3::Z)).unwrap();
        }
        let inner = source.faces().find(|(id, _)| (source.face_area(*id) - 4.).abs() < EPS).unwrap().0;
        source.erase(&[Element::Face(inner)]);
        let face = source.faces().next().unwrap().0;
        let placement = TexturePlacement { offset: [0.2, 0.4], scale: [2., 3.], rotation: 0.3, ..Default::default() };
        let polygon = source.faces.get_mut(&face).unwrap();
        polygon.material = Some(MaterialId(41));
        polygon.back_material = Some(MaterialId(42));
        polygon.uv = Some(placement);
        polygon.hidden = true;
        for edge in source.edges.values_mut() {
            edge.soft = true;
            edge.smooth = true;
            edge.hidden = true;
            edge.material = Some(MaterialId(43));
        }
        let polygon = source.face(face).unwrap();
        let curve_edges = polygon
            .outer
            .iter()
            .zip(polygon.outer.iter().cycle().skip(1))
            .take(4)
            .map(|(a, b)| source.edge_between(*a, *b).unwrap())
            .collect::<Vec<_>>();
        source
            .register_curve(
                &curve_edges,
                Curve {
                    kind: CurveKind::Polyline,
                    faceted: false,
                    centre: DVec3::ZERO,
                    normal: DVec3::Z,
                    start: DVec3::X,
                    radius: 0.,
                    segments: 4,
                    sweep: 0.,
                },
            )
            .unwrap();
        let expected_normal = source.face_normal(face).unwrap();
        let template = Template::whole(&source);
        let mut target = Mesh::new();
        target.stamp(&template, |p| DVec3::new(10. - p.x, 2. + p.y, 3. + p.z)).unwrap();
        target.validate().unwrap();
        assert_eq!(target.face_count(), 1);
        let (id, polygon) = target.faces().next().unwrap();
        assert_eq!(polygon.holes.len(), 1);
        assert!((target.face_area(id) - 12.).abs() < EPS);
        let normal = target.face_normal(id).unwrap();
        assert!(normal.dot(expected_normal) > 0.99, "source {expected_normal:?}, target {normal:?}");
        assert_eq!(polygon.material, Some(MaterialId(41)));
        assert_eq!(polygon.back_material, Some(MaterialId(42)));
        assert_eq!(polygon.uv, Some(placement));
        assert!(polygon.hidden);
        assert!(target.edges().all(|(_, e)| e.soft && e.smooth && e.hidden && e.material == Some(MaterialId(43))));
        let (curve, value) = target.curves().next().unwrap();
        assert_eq!(target.curves().count(), 1);
        assert_eq!(value.kind, CurveKind::Polyline);
        assert_eq!(target.curve_edges(curve).len(), 4);
    }
}
