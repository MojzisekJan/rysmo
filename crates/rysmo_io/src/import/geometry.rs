use super::{Result, error};
use rysmo_math::{DVec2, DVec3, EPS, Plane};
use rysmo_mesh::{Edge, EdgeId, Face, FaceId, MaterialId, Mesh, TexturePlacement, Vertex, VertexId};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Geometry {
    next: u64,
    vertices: BTreeMap<VertexId, Vertex>,
    positions: BTreeMap<[i64; 3], VertexId>,
    edges: BTreeMap<EdgeId, Edge>,
    edge_keys: BTreeMap<(VertexId, VertexId), EdgeId>,
    faces: BTreeMap<FaceId, Face>,
    face_keys: BTreeMap<[VertexId; 3], FaceId>,
    normals: BTreeMap<EdgeId, Vec<DVec3>>,
}
impl Geometry {
    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
    fn vertex(&mut self, p: DVec3) -> Result<VertexId> {
        if !p.is_finite() || p.abs().max_element() > 1e9 {
            return error("Non-finite or excessive coordinates.");
        }
        let key = p.to_array().map(|v| (v / (EPS * 0.01)).round() as i64);
        if let Some(id) = self.positions.get(&key) {
            return Ok(*id);
        }
        if self.vertices.len() >= 200_000 {
            return error("Import exceeds 200,000 vertices.");
        }
        let id = VertexId(self.id());
        self.vertices.insert(id, Vertex { pos: p });
        self.positions.insert(key, id);
        Ok(id)
    }
    fn edge(&mut self, a: VertexId, b: VertexId) -> EdgeId {
        let key = (a.min(b), a.max(b));
        if let Some(id) = self.edge_keys.get(&key) {
            return *id;
        }
        let id = EdgeId(self.id());
        self.edges.insert(id, Edge::new(a, b));
        self.edge_keys.insert(key, id);
        id
    }
    pub fn line(&mut self, a: DVec3, b: DVec3) -> Result<()> {
        if a.distance(b) < EPS {
            return Ok(());
        }
        if self.edges.len() >= 300_000 {
            return error("Import exceeds 300,000 edges.");
        }
        let a = self.vertex(a)?;
        let b = self.vertex(b)?;
        if a != b {
            self.edge(a, b);
        }
        Ok(())
    }
    pub fn polygon(&mut self, points: &[DVec3], uv: Option<&[DVec2]>, material: Option<MaterialId>) -> Result<()> {
        if points.len() < 3 || points.len() > 16_384 {
            return error("Invalid polygon vertex count.");
        }
        if uv.is_some_and(|uv| uv.len() != points.len() || uv.iter().any(|p| !p.is_finite())) {
            return error("Invalid polygon UV coordinates.");
        }
        let plane = Plane::from_polygon(points).ok_or_else(|| super::ImportError("Degenerate polygon.".into()))?;
        let mut triangles = Vec::<u32>::new();
        earcut::Earcut::new().earcut(points.iter().map(|p| plane.to_2d(*p).to_array()), &[], &mut triangles);
        if triangles.is_empty() {
            return error("Polygon could not be triangulated.");
        }
        for triangle in triangles.as_chunks::<3>().0 {
            let mut indices = triangle.map(|i| i as usize);
            let mut p = indices.map(|i| points[i]);
            if (p[1] - p[0]).cross(p[2] - p[0]).dot(plane.normal) < 0. {
                indices.swap(1, 2);
                p.swap(1, 2);
            }
            let normal = (p[1] - p[0]).cross(p[2] - p[0]).try_normalize();
            let Some(normal) = normal else { continue };
            let ids = [self.vertex(p[0])?, self.vertex(p[1])?, self.vertex(p[2])?];
            if ids[0] == ids[1] || ids[1] == ids[2] || ids[2] == ids[0] {
                continue;
            }
            let mut key = ids;
            key.sort();
            if let Some(id) = self.face_keys.get(&key) {
                if let Some(face) = self.faces.get_mut(id) {
                    face.back_material = material.or(face.back_material);
                }
                continue;
            }
            if self.faces.len() >= 100_000 {
                return error("Import exceeds 100,000 triangles.");
            }
            for i in 0..3 {
                let edge = self.edge(ids[i], ids[(i + 1) % 3]);
                self.normals.entry(edge).or_default().push(normal);
            }
            let placement = uv.and_then(|uv| texture_placement(p, indices.map(|i| uv[i]), normal));
            let id = FaceId(self.id());
            self.faces.insert(
                id,
                Face {
                    outer: ids.to_vec(),
                    holes: Vec::new(),
                    material,
                    back_material: material,
                    uv: placement,
                    hidden: false,
                },
            );
            self.face_keys.insert(key, id);
        }
        Ok(())
    }
    pub fn finish(self) -> Result<Mesh> {
        let mut mesh = Mesh::from_parts(self.vertices, self.edges, self.faces)?;
        for (id, normals) in self.normals {
            if let [a, b] = normals.as_slice() {
                mesh.set_edge_flags(
                    &[id],
                    Some(a.dot(*b) > 1. - 1e-8),
                    Some(a.dot(*b) > 30_f64.to_radians().cos()),
                    None,
                );
            }
        }
        Ok(mesh)
    }
}
/// An affine UV map represented by four world-plane tile corners.
fn texture_placement(p: [DVec3; 3], uv: [DVec2; 3], normal: DVec3) -> Option<TexturePlacement> {
    let a = uv[1] - uv[0];
    let b = uv[2] - uv[0];
    let determinant = a.perp_dot(b);
    if determinant.abs() < 1e-12 {
        return None;
    }
    let (u, v) = rysmo_model::texture_axes(normal);
    let corners = [DVec2::ZERO, DVec2::X, DVec2::ONE, DVec2::Y].map(|q| {
        let q = q - uv[0];
        let world =
            p[0] + (p[1] - p[0]) * (q.perp_dot(b) / determinant) + (p[2] - p[0]) * (a.perp_dot(q) / determinant);
        [u.dot(world), -v.dot(world)]
    });
    let placement = TexturePlacement { pins: Some(corners), ..Default::default() };
    rysmo_model::valid_placement(&placement).then_some(placement)
}
