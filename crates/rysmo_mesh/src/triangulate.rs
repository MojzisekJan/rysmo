//! Triangulating faces for drawing and export.

use rysmo_math::DVec2;

use crate::ids::{FaceId, VertexId};
use crate::mesh::Mesh;

/// Triangles covering a face, wound counter-clockwise seen from its front.
pub fn triangulate_face(mesh: &Mesh, f: FaceId) -> Vec<[VertexId; 3]> {
    let (Some(face), Some(plane)) = (mesh.face(f), mesh.face_plane(f)) else { return Vec::new() };
    let mut ids: Vec<VertexId> = Vec::new();
    let mut pts: Vec<DVec2> = Vec::new();
    let mut holes: Vec<u32> = Vec::new();
    for (i, l) in face.loops().enumerate() {
        if i > 0 {
            holes.push(ids.len() as u32);
        }
        for v in l {
            ids.push(*v);
            pts.push(plane.to_2d(mesh.pos(*v)));
        }
    }
    let mut tris: Vec<u32> = Vec::new();
    earcut::Earcut::new().earcut(pts.iter().map(|p| [p.x, p.y]), &holes, &mut tris);
    tris.as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let ccw = (pts[b] - pts[a]).perp_dot(pts[c] - pts[a]) > 0.0;
            if ccw { [ids[a], ids[b], ids[c]] } else { [ids[a], ids[c], ids[b]] }
        })
        .collect()
}
