//! The mesh container, its elements, and the low-level edits the operations build on.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rysmo_math::{DVec3, EPS, Plane};
use serde::{Deserialize, Serialize};

use crate::ids::{EdgeId, Element, FaceId, MaterialId, VertexId};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub pos: DVec3,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// An edge between two vertices. Soft edges are not drawn, smooth edges blend the
/// shading of the faces on either side, hidden edges are neither drawn nor picked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// Evaluated component-opening boundary, owned by the component rather than the host.
    #[serde(skip)]
    pub procedural: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<MaterialId>,
    pub v: [VertexId; 2],
    #[serde(default, skip_serializing_if = "is_false")]
    pub soft: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<crate::CurveId>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub smooth: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
}

impl Edge {
    pub fn new(a: VertexId, b: VertexId) -> Self {
        Edge { procedural: false, material: None, v: [a, b], soft: false, smooth: false, hidden: false, curve: None }
    }

    /// Whether the viewport draws it and tools can pick it.
    pub fn visible(&self) -> bool {
        !self.soft && !self.hidden
    }

    pub(crate) fn flags(&self) -> (bool, bool, bool, Option<MaterialId>) {
        (self.soft, self.smooth, self.hidden, self.material)
    }

    pub(crate) fn set_flags(&mut self, (soft, smooth, hidden, material): (bool, bool, bool, Option<MaterialId>)) {
        self.soft = soft;
        self.smooth = smooth;
        self.hidden = hidden;
        self.material = material;
    }
}

/// Where a textured material sits on a face: shifted by `offset` (metres along the
/// face's texture axes), turned by `rotation` degrees, and scaled by `scale` (1 is the
/// material's own size). The texture axes are the face plane's axes lined up with the
/// model axes (see `rysmo_math::model_axes`), so textures on neighbouring faces line
/// up until moved.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TexturePlacement {
    #[serde(default)]
    pub offset: [f64; 2],
    #[serde(default)]
    pub rotation: f64,
    #[serde(default = "unit_scale")]
    pub scale: [f64; 2],
    /// Corners of one texture tile in the unwarped UV plane, clockwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pins: Option<[[f64; 2]; 4]>,
    /// A shared projection direction instead of each target face normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projected_normal: Option<[f64; 3]>,
}

fn unit_scale() -> [f64; 2] {
    [1.0, 1.0]
}

impl Default for TexturePlacement {
    fn default() -> Self {
        TexturePlacement { offset: [0.0, 0.0], rotation: 0.0, scale: [1.0, 1.0], pins: None, projected_normal: None }
    }
}

/// A planar face. The outer loop runs counter-clockwise seen from the front (the side
/// the normal points to); holes run clockwise. Consecutive loop vertices are joined by
/// edges, including the last back to the first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    #[serde(default)]
    pub hidden: bool,
    pub outer: Vec<VertexId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<VertexId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<MaterialId>,
    /// The material of the back side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back_material: Option<MaterialId>,
    /// How a textured material sits on the face; `None` is the default placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv: Option<TexturePlacement>,
}

impl Face {
    pub fn loops(&self) -> impl Iterator<Item = &Vec<VertexId>> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }

    /// The edges of all loops as vertex pairs in loop order.
    pub fn loop_edges(&self) -> Vec<(VertexId, VertexId)> {
        let mut out = Vec::new();
        for l in self.loops() {
            for i in 0..l.len() {
                out.push((l[i], l[(i + 1) % l.len()]));
            }
        }
        out
    }

    pub fn has_vertex(&self, v: VertexId) -> bool {
        self.loops().any(|l| l.contains(&v))
    }

    pub(crate) fn reverse(&mut self) {
        self.outer.reverse();
        for h in &mut self.holes {
            h.reverse();
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MeshError {
    #[error("no such element: {0}")]
    Missing(u64),
    #[error("the line has no length")]
    ZeroLength,
    #[error("the push/pull distance is zero")]
    ZeroDistance,
    #[error("the move would bend face {0} out of its plane")]
    WouldBend(FaceId),
    #[error("the offset does not fit the shape")]
    OffsetTooLarge,
    #[error("the edges do not form one chain")]
    NotAChain,
    #[error("{0}")]
    Unsupported(String),
    #[error("invalid mesh: {0}")]
    Invalid(String),
}

/// Vertices, edges and faces, keyed by stable ids.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Mesh {
    pub(crate) vertices: BTreeMap<VertexId, Vertex>,
    pub(crate) edges: BTreeMap<EdgeId, Edge>,
    pub(crate) faces: BTreeMap<FaceId, Face>,
    #[serde(default)]
    pub(crate) curves: BTreeMap<crate::CurveId, crate::Curve>,
    pub(crate) next_id: u64,
    #[serde(skip)]
    pub(crate) edge_index: HashMap<(VertexId, VertexId), EdgeId>,
}

impl<'de> Deserialize<'de> for Mesh {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Stored {
            vertices: BTreeMap<VertexId, Vertex>,
            edges: BTreeMap<EdgeId, Edge>,
            faces: BTreeMap<FaceId, Face>,
            #[serde(default)]
            curves: BTreeMap<crate::CurveId, crate::Curve>,
            next_id: u64,
        }
        let stored = Stored::deserialize(deserializer)?;
        let highest = stored
            .vertices
            .keys()
            .map(|id| id.0)
            .chain(stored.edges.keys().map(|id| id.0))
            .chain(stored.faces.keys().map(|id| id.0))
            .chain(stored.curves.keys().map(|id| id.0))
            .max()
            .unwrap_or(0);
        let minimum = if highest == 0 {
            0
        } else {
            highest.checked_add(1).ok_or_else(|| serde::de::Error::custom("Mesh identifier overflow"))?
        };
        let mut mesh = Mesh {
            vertices: stored.vertices,
            edges: stored.edges,
            faces: stored.faces,
            curves: stored.curves,
            next_id: stored.next_id.max(minimum),
            edge_index: HashMap::new(),
        };
        mesh.reindex();
        mesh.validate().map_err(serde::de::Error::custom)?;
        Ok(mesh)
    }
}

pub(crate) fn key(a: VertexId, b: VertexId) -> (VertexId, VertexId) {
    if a < b { (a, b) } else { (b, a) }
}

impl Mesh {
    pub fn new() -> Self {
        Mesh { next_id: 1, ..Default::default() }
    }

    /// Rebuild a mesh with globally fresh identifiers while retaining exact topology.
    pub fn with_fresh_ids(&self, mut allocate: impl FnMut() -> u64) -> Result<Mesh, MeshError> {
        Mesh::from_parts_with_curves(
            self.vertices.clone(),
            self.edges.clone(),
            self.faces.clone(),
            self.curves.clone(),
        )?;
        let vertices: BTreeMap<_, _> = self.vertices.keys().map(|id| (*id, VertexId(allocate()))).collect();
        let curves: BTreeMap<_, _> = self.curves.keys().map(|id| (*id, crate::CurveId(allocate()))).collect();
        let v = self.vertices.iter().map(|(id, v)| (vertices[id], v.clone())).collect();
        let e = self
            .edges
            .values()
            .map(|e| {
                let mut e = e.clone();
                e.v = e.v.map(|v| vertices[&v]);
                e.curve = e.curve.map(|id| curves[&id]);
                (EdgeId(allocate()), e)
            })
            .collect();
        let f = self
            .faces
            .values()
            .map(|f| {
                let mut f = f.clone();
                f.outer = f.outer.iter().map(|id| vertices[id]).collect();
                f.holes = f.holes.iter().map(|h| h.iter().map(|id| vertices[id]).collect()).collect();
                (FaceId(allocate()), f)
            })
            .collect();
        let c = self.curves.iter().map(|(id, c)| (curves[id], *c)).collect();
        Mesh::from_parts_with_curves(v, e, f, c)
    }

    /// Builds a mesh from stored elements (for loading files) and checks it.
    pub fn from_parts(
        vertices: BTreeMap<VertexId, Vertex>,
        edges: BTreeMap<EdgeId, Edge>,
        faces: BTreeMap<FaceId, Face>,
    ) -> Result<Self, MeshError> {
        Self::from_parts_with_curves(vertices, edges, faces, BTreeMap::new())
    }

    pub fn from_parts_with_curves(
        vertices: BTreeMap<VertexId, Vertex>,
        edges: BTreeMap<EdgeId, Edge>,
        faces: BTreeMap<FaceId, Face>,
        curves: BTreeMap<crate::CurveId, crate::Curve>,
    ) -> Result<Self, MeshError> {
        let max = vertices
            .keys()
            .map(|k| k.0)
            .chain(edges.keys().map(|k| k.0))
            .chain(faces.keys().map(|k| k.0))
            .chain(curves.keys().map(|k| k.0))
            .max()
            .unwrap_or(0);
        let mut m = Mesh { vertices, edges, faces, curves, next_id: max + 1, edge_index: HashMap::new() };
        m.reindex();
        m.validate()?;
        Ok(m)
    }

    /// The next id the mesh will hand out; documents share it with other element kinds.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Makes sure ids handed out from now on are at least `at_least`, for documents that
    /// also give ids to elements outside the mesh (materials).
    pub fn ensure_next_id(&mut self, at_least: u64) {
        self.next_id = self.next_id.max(at_least);
    }

    pub fn reserve_id(&mut self) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    pub(crate) fn reindex(&mut self) {
        self.edge_index = self.edges.iter().map(|(id, e)| (key(e.v[0], e.v[1]), *id)).collect();
    }

    /// Checks the invariants: edges join two distinct existing vertices, no two edges
    /// join the same pair, every loop step is an edge, and faces are planar.
    pub fn validate(&self) -> Result<(), MeshError> {
        let mut seen = BTreeSet::new();
        for (id, curve) in &self.curves {
            curve.validate().map_err(|e| MeshError::Invalid(format!("curve {id}: {e}")))?;
        }
        for (id, e) in &self.edges {
            if e.curve.is_some_and(|c| !self.curves.contains_key(&c)) {
                return Err(MeshError::Invalid(format!("edge {id} references a missing curve")));
            }
            if e.v[0] == e.v[1] {
                return Err(MeshError::Invalid(format!("edge {id} is a loop on one vertex")));
            }
            for v in e.v {
                if !self.vertices.contains_key(&v) {
                    return Err(MeshError::Invalid(format!("edge {id} uses missing vertex {v}")));
                }
            }
            if !seen.insert(key(e.v[0], e.v[1])) {
                return Err(MeshError::Invalid(format!("edge {id} duplicates another edge")));
            }
        }
        for (id, f) in &self.faces {
            if f.outer.len() < 3 {
                return Err(MeshError::Invalid(format!("face {id} has fewer than three vertices")));
            }
            for (a, b) in f.loop_edges() {
                if self.edge_between(a, b).is_none() {
                    return Err(MeshError::Invalid(format!("face {id} steps from {a} to {b} without an edge")));
                }
            }
            let plane = self.face_plane(*id).ok_or_else(|| MeshError::Invalid(format!("face {id} is degenerate")))?;
            for l in f.loops() {
                for v in l {
                    if plane.distance(self.pos(*v)).abs() > EPS * 10.0 {
                        return Err(MeshError::Invalid(format!("face {id} is not planar")));
                    }
                }
            }
        }
        Ok(())
    }

    // ----- reading -----

    pub fn vertices(&self) -> impl Iterator<Item = (VertexId, &Vertex)> {
        self.vertices.iter().map(|(k, v)| (*k, v))
    }

    pub fn edges(&self) -> impl Iterator<Item = (EdgeId, &Edge)> {
        self.edges.iter().map(|(k, v)| (*k, v))
    }

    pub fn faces(&self) -> impl Iterator<Item = (FaceId, &Face)> {
        self.faces.iter().map(|(k, v)| (*k, v))
    }

    pub fn vertex(&self, id: VertexId) -> Option<&Vertex> {
        self.vertices.get(&id)
    }

    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(&id)
    }

    pub fn face(&self, id: FaceId) -> Option<&Face> {
        self.faces.get(&id)
    }

    pub fn face_mut(&mut self, id: FaceId) -> Option<&mut Face> {
        self.faces.get_mut(&id)
    }

    pub fn contains(&self, e: Element) -> bool {
        match e {
            Element::Edge(id) => self.edges.contains_key(&id),
            Element::Face(id) => self.faces.contains_key(&id),
            Element::Instance(_)
            | Element::Guide(_)
            | Element::Dimension(_)
            | Element::Text(_)
            | Element::Section(_) => false,
        }
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Makes an evaluated opening ordinary editable boundary geometry.
    pub fn materialize_opening_edges(&mut self) {
        for edge in self.edges.values_mut().filter(|edge| edge.procedural) {
            edge.procedural = false;
            edge.hidden = false;
        }
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// The position of a vertex. Panics on a missing id, which is a bug in the caller.
    pub fn pos(&self, v: VertexId) -> DVec3 {
        self.vertices[&v].pos
    }

    pub fn edge_points(&self, e: EdgeId) -> (DVec3, DVec3) {
        let e = &self.edges[&e];
        (self.pos(e.v[0]), self.pos(e.v[1]))
    }

    pub fn edge_between(&self, a: VertexId, b: VertexId) -> Option<EdgeId> {
        self.edge_index.get(&key(a, b)).copied()
    }

    /// The faces whose loops run along an edge.
    pub fn faces_of_edge(&self, e: EdgeId) -> Vec<FaceId> {
        let Some(edge) = self.edges.get(&e) else { return Vec::new() };
        let k = key(edge.v[0], edge.v[1]);
        self.faces
            .iter()
            .filter(|(_, f)| f.loop_edges().iter().any(|(a, b)| key(*a, *b) == k))
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn edges_of_vertex(&self, v: VertexId) -> Vec<EdgeId> {
        self.edges.iter().filter(|(_, e)| e.v.contains(&v)).map(|(id, _)| *id).collect()
    }

    pub fn faces_of_vertex(&self, v: VertexId) -> Vec<FaceId> {
        self.faces.iter().filter(|(_, f)| f.has_vertex(v)).map(|(id, _)| *id).collect()
    }

    /// The edges that bound a face.
    pub fn face_edges(&self, f: FaceId) -> Vec<EdgeId> {
        self.faces[&f].loop_edges().into_iter().filter_map(|(a, b)| self.edge_between(a, b)).collect()
    }

    pub fn loop_points(&self, l: &[VertexId]) -> Vec<DVec3> {
        l.iter().map(|v| self.pos(*v)).collect()
    }

    /// The face's plane, facing the way its normal points.
    pub fn face_plane(&self, f: FaceId) -> Option<Plane> {
        let face = self.faces.get(&f)?;
        Plane::from_polygon(&self.loop_points(&face.outer))
    }

    pub fn face_normal(&self, f: FaceId) -> Option<DVec3> {
        self.face_plane(f).map(|p| p.normal)
    }

    /// The face's area: the outer loop's minus the holes'.
    pub fn face_area(&self, f: FaceId) -> f64 {
        let Some(plane) = self.face_plane(f) else { return 0.0 };
        let face = &self.faces[&f];
        let area = |l: &[VertexId]| {
            let pts: Vec<_> = l.iter().map(|v| plane.to_2d(self.pos(*v))).collect();
            rysmo_math::polygon_area_2d(&pts).abs()
        };
        area(&face.outer) - face.holes.iter().map(|h| area(h)).sum::<f64>()
    }

    /// The vertex at `p`, if there is one within the tolerance.
    pub fn find_vertex(&self, p: DVec3) -> Option<VertexId> {
        self.vertices.iter().find(|(_, v)| v.pos.distance(p) < EPS).map(|(id, _)| *id)
    }

    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut it = self.vertices.values().map(|v| v.pos);
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// The vertices an element set touches.
    pub fn vertices_of(&self, elements: &[Element]) -> BTreeSet<VertexId> {
        let mut out = BTreeSet::new();
        for e in elements {
            match e {
                Element::Edge(id) => {
                    if let Some(edge) = self.edges.get(id) {
                        out.extend(edge.v);
                    }
                }
                Element::Face(id) => {
                    if let Some(f) = self.faces.get(id) {
                        for l in f.loops() {
                            out.extend(l.iter().copied());
                        }
                    }
                }
                Element::Instance(_)
                | Element::Guide(_)
                | Element::Dimension(_)
                | Element::Text(_)
                | Element::Section(_) => {}
            }
        }
        out
    }

    // ----- low-level edits -----

    /// The vertex at `p`, created if there is none there yet.
    pub fn add_vertex(&mut self, p: DVec3) -> VertexId {
        if let Some(v) = self.find_vertex(p) {
            return v;
        }
        let id = VertexId(self.reserve_id());
        self.vertices.insert(id, Vertex { pos: p });
        id
    }

    /// The edge between two vertices, created if missing. No splitting or face work.
    pub(crate) fn raw_edge(&mut self, a: VertexId, b: VertexId) -> EdgeId {
        debug_assert_ne!(a, b);
        if let Some(e) = self.edge_between(a, b) {
            return e;
        }
        let id = EdgeId(self.reserve_id());
        self.edges.insert(id, Edge::new(a, b));
        self.edge_index.insert(key(a, b), id);
        id
    }

    pub(crate) fn raw_face(&mut self, face: Face) -> FaceId {
        let id = FaceId(self.reserve_id());
        self.faces.insert(id, face);
        id
    }

    pub(crate) fn remove_edge_raw(&mut self, e: EdgeId) {
        if let Some(edge) = self.edges.remove(&e) {
            self.edge_index.remove(&key(edge.v[0], edge.v[1]));
        }
    }

    pub fn remove_face(&mut self, f: FaceId) -> bool {
        self.faces.remove(&f).is_some()
    }

    /// Removes vertices no edge uses.
    pub(crate) fn remove_orphan_vertices(&mut self) {
        let used: BTreeSet<VertexId> = self.edges.values().flat_map(|e| e.v).collect();
        self.vertices.retain(|id, _| used.contains(id));
    }

    /// Splits edge `e` at vertex `v`, which must lie on it, and threads `v` into the
    /// loops of the faces along it. Returns the two halves.
    pub(crate) fn split_edge(&mut self, e: EdgeId, v: VertexId) -> (EdgeId, EdgeId) {
        let [a, b] = self.edges[&e].v;
        let flags = self.edges[&e].flags();
        let curve = self.edges[&e].curve;
        self.remove_edge_raw(e);
        let e1 = self.raw_edge(a, v);
        let e2 = self.raw_edge(v, b);
        for h in [e1, e2] {
            let edge = self.edges.get_mut(&h).expect("just made");
            edge.set_flags(flags);
            edge.curve = curve;
        }
        for f in self.faces.values_mut() {
            for l in std::iter::once(&mut f.outer).chain(f.holes.iter_mut()) {
                let n = l.len();
                let mut i = 0;
                while i < l.len() {
                    let x = l[i];
                    let y = l[(i + 1) % l.len()];
                    if (x == a && y == b) || (x == b && y == a) {
                        l.insert(i + 1, v);
                        break;
                    }
                    i += 1;
                }
                debug_assert!(l.len() <= n + 1);
            }
        }
        (e1, e2)
    }

    /// Replaces vertex `from` by `to` everywhere, dropping edges and loop steps that
    /// collapse. Used when a move lands one vertex on another.
    pub(crate) fn weld(&mut self, from: VertexId, to: VertexId) {
        if from == to {
            return;
        }
        let affected: Vec<EdgeId> = self.edges_of_vertex(from);
        for e in affected {
            let [a, b] = self.edges[&e].v;
            let flags = self.edges[&e].flags();
            let curve = self.edges[&e].curve;
            let other = if a == from { b } else { a };
            self.remove_edge_raw(e);
            if other != to {
                let n = self.raw_edge(other, to);
                let edge = self.edges.get_mut(&n).expect("just made");
                edge.set_flags(flags);
                edge.curve = curve;
            }
        }
        for f in self.faces.values_mut() {
            for l in std::iter::once(&mut f.outer).chain(f.holes.iter_mut()) {
                for v in l.iter_mut() {
                    if *v == from {
                        *v = to;
                    }
                }
                l.dedup();
                while l.len() > 1 && l.first() == l.last() {
                    l.pop();
                }
            }
            f.holes.retain(|h| h.len() >= 3);
        }
        self.faces.retain(|_, f| f.outer.len() >= 3);
        self.vertices.remove(&from);
    }
}
