//! Wavefront OBJ export.
//!
//! Faces without holes are written as polygons; faces with holes are triangulated, as
//! OBJ has no holes. Each face gets its normal, and texture coordinates where its
//! material is textured. Materials go to a companion MTL file, their images beside it.

use std::collections::BTreeMap;
use std::fmt::Write;

use rysmo_mesh::{VertexId, triangulate_face};
use rysmo_model::Document;

use crate::y_up;

/// An OBJ export: the OBJ text, the MTL text, and the texture images the MTL names
/// (file name and bytes), to be written beside them.
#[must_use]
pub struct ObjExport {
    pub obj: String,
    pub mtl: String,
    pub images: Vec<(String, Vec<u8>)>,
}

/// Content-addressed companion name, independent of user-supplied paths.
pub(crate) fn image_file(doc: &Document, id: rysmo_mesh::ImageId) -> String {
    let image = &doc.images[&id];
    let ext = if image.mime.contains("jpeg") { "jpg" } else { "png" };
    format!("texture-{}-{:08x}.{ext}", id.0, crc32fast::hash(&image.bytes))
}

/// Write companions before the OBJ. A conflicting image or orphaned MTL is
/// refused before any output is replaced; identical shared images are reused.
pub fn write(path: &std::path::Path, output: &ObjExport) -> std::io::Result<()> {
    use std::{fs, io};
    let directory = path.parent().unwrap_or(std::path::Path::new("."));
    let mtl = path.with_extension("mtl");
    let same_or_absent = |path: &std::path::Path, bytes: &[u8]| -> io::Result<bool> {
        match fs::read(path) {
            Ok(existing) if existing == bytes => Ok(true),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("Export companion {} contains different data. Choose another folder.", path.display()),
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    };
    if !path.try_exists()? {
        same_or_absent(&mtl, output.mtl.as_bytes())?;
    }
    let mut missing = Vec::new();
    for (name, bytes) in &output.images {
        if std::path::Path::new(name).file_name().is_none_or(|n| n != name.as_str()) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid companion filename"));
        }
        let image = directory.join(name);
        if !same_or_absent(&image, bytes)? {
            missing.push((image, bytes));
        }
    }
    for (image, bytes) in missing {
        crate::atomic::write(&image, bytes)?;
    }
    crate::atomic::write(&mtl, output.mtl.as_bytes())?;
    crate::atomic::write(path, output.obj.as_bytes())
}

/// Exports the model as OBJ. `mtl_name` is the file name the OBJ refers to. Groups and
/// components are flattened into world coordinates, one OBJ group each.
pub fn export(doc: &Document, mtl_name: &str) -> ObjExport {
    let mut uv_count = 0usize;
    let mut obj = String::from("# Rysmo export, Y up, metres\n");
    let _ = writeln!(obj, "mtllib {mtl_name}");
    let mut base = 0usize;
    let mut normal_count = 0;
    for (k, occ) in doc.occurrences(&[]).iter().enumerate() {
        let Some(def) = doc.def(occ.definition) else { continue };
        let evaluated = doc.effective_mesh(occ.definition);
        let mesh = &*evaluated;
        if mesh.face_count() == 0 {
            continue;
        }
        let t = occ.transform;
        let mirrored = t.determinant() < 0.0;
        let nt = t.inverse().transpose();
        let name =
            if occ.definition.is_none() { "Model".to_string() } else { format!("{}_{}", sanitise(&def.name), k) };
        let _ = writeln!(obj, "g {name}");
        let mut index: BTreeMap<VertexId, usize> = BTreeMap::new();
        for (i, (id, v)) in mesh.vertices().enumerate() {
            let p = y_up(t.transform_point3(v.pos));
            let _ = writeln!(obj, "v {} {} {}", p[0], p[1], p[2]);
            index.insert(id, base + i + 1);
        }
        base += mesh.vertex_count();
        let mut current: Option<String> = None;
        for (fid, face) in mesh.faces().filter(|(_, face)| !face.hidden) {
            let Some(n) = mesh.face_normal(fid) else { continue };
            let n = y_up(nt.transform_vector3(n).normalize());
            let _ = writeln!(obj, "vn {} {} {}", n[0], n[1], n[2]);
            normal_count += 1;
            let mat = material_name(doc, face.material.or(occ.material));
            if current.as_deref() != Some(mat.as_str()) {
                let _ = writeln!(obj, "usemtl {mat}");
                current = Some(mat);
            }
            let polys: Vec<Vec<VertexId>> = if face.holes.is_empty() {
                vec![face.outer.clone()]
            } else {
                triangulate_face(mesh, fid).into_iter().map(|t| t.to_vec()).collect()
            };
            let texture = face.material.or(occ.material).and_then(|m| doc.materials.get(&m)).and_then(|m| m.texture);
            let local_n = mesh.face_normal(fid).unwrap_or_default();
            let placement = face.uv.unwrap_or_default();
            for mut p in polys {
                if mirrored {
                    p.reverse();
                }
                if let Some(tex) = texture {
                    let first = uv_count + 1;
                    for v in &p {
                        let uv = rysmo_model::uv_at(mesh.pos(*v), local_n, tex.size, &placement);
                        // OBJ's v grows upwards.
                        let _ = writeln!(obj, "vt {} {}", uv[0], -uv[1]);
                    }
                    uv_count += p.len();
                    obj.push('f');
                    for (k, v) in p.iter().enumerate() {
                        let _ = write!(obj, " {}/{}/{}", index[v], first + k, normal_count);
                    }
                } else {
                    obj.push('f');
                    for v in p {
                        let _ = write!(obj, " {}//{}", index[&v], normal_count);
                    }
                }
                obj.push('\n');
            }
        }
    }
    let mut mtl = String::from("# Rysmo materials\nnewmtl Default\nKd 1 1 1\n");
    let mut images = Vec::new();
    for m in doc.materials.values() {
        let _ = writeln!(
            mtl,
            "\nnewmtl {}\nKd {} {} {}\nd {}",
            sanitise(&m.name),
            m.color[0],
            m.color[1],
            m.color[2],
            m.color[3]
        );
        if let Some(t) = m.texture
            && doc.images.contains_key(&t.image)
        {
            let file = image_file(doc, t.image);
            let _ = writeln!(mtl, "map_Kd {file}");
            if !images.iter().any(|(f, _): &(String, Vec<u8>)| *f == file) {
                images.push((file, doc.images[&t.image].bytes.as_ref().clone()));
            }
        }
    }
    ObjExport { obj, mtl, images }
}

fn material_name(doc: &Document, m: Option<rysmo_mesh::MaterialId>) -> String {
    m.and_then(|m| doc.materials.get(&m)).map(|m| sanitise(&m.name)).unwrap_or_else(|| "Default".into())
}

pub(crate) fn sanitise(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect();
    if s.is_empty() { "Unnamed".into() } else { s }
}

#[cfg(test)]
mod tests {
    use rysmo_math::DVec3;
    use rysmo_model::{Command, Editor};

    #[test]
    fn box_exports_six_quads() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let f = e.mesh().faces().next().unwrap().0;
        e.apply(Command::PushPull { face: f, distance: -1.0, keep_base: false }).unwrap();
        let super::ObjExport { obj, mtl, .. } = super::export(e.doc(), "box.mtl");
        assert_eq!(obj.lines().filter(|l| l.starts_with("v ")).count(), 8);
        assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), 6);
        assert!(obj.lines().filter(|l| l.starts_with("f ")).all(|l| l.split_whitespace().count() == 5));
        assert!(mtl.contains("newmtl Default"));
    }

    #[test]
    fn textured_faces_get_uvs_and_the_image() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let f = e.mesh().faces().next().unwrap().0;
        let img = e
            .apply(Command::AddImage { name: "Brick wall.png".into(), mime: "image/png".into(), data: vec![1, 2, 3] })
            .unwrap()
            .image
            .unwrap();
        let m = e
            .apply(Command::AddMaterial {
                name: "Brick".into(),
                color: [0.6, 0.3, 0.2, 1.0],
                texture: Some(rysmo_model::Texture { image: img, size: [0.5, 0.25] }),
            })
            .unwrap()
            .material;
        e.apply(Command::Paint { faces: vec![f], material: m, back: false }).unwrap();
        let out = super::export(e.doc(), "m.mtl");
        assert_eq!(out.obj.lines().filter(|l| l.starts_with("vt ")).count(), 4);
        assert!(out.obj.lines().any(|l| l.starts_with("f ")
            && l.split_whitespace().nth(1).unwrap().split('/').count() == 3
            && !l.contains("//")));
        let file = super::image_file(e.doc(), img);
        assert!(out.mtl.contains(&format!("map_Kd {file}")));
        assert_eq!(out.images, vec![(file, vec![1, 2, 3])]);
    }
}
